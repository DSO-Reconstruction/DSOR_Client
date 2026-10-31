//! Mounts: a ride status effect puts its mount under its holder.
//!
//! The server runs the whole flow (DSOR_Server services/customization.py
//! `_toggle_mount_definition`): using a mount -- from the bag (InventoryItemAction
//! 0x13) or the sticker book (UseStickerBookItemCommand) -- starts its cast effect
//! (cast_mount_*: the holder channels, `hometown_teleport`, drawn by
//! crate::gameplay::skills' status visuals), then its ride effect (ride_mount_*),
//! whose StartModifiers name the mount ("Mounted:Mount,<id>") and its
//! MovementBonus. Using it again, or casting a skill, removes the ride.
//!
//! Here: while a holder has a ride effect, its mount model (_Template_Mount
//! Graphics with its CharacterSet, crate::actors::monsters::spawn_model) stands
//! under it, playing its anim set's Idle or Run with the rider; the rider sits on
//! the mount's `mount` joint and plays the same anim set -- its Idle / Run clips
//! have rider twins of the same name in the uniskel skeletons
//! (mount_idle_polarbear_01-loop in both polarbear.glb and uniskel/__animations).
//! The local player walks RUN_SPEED + the bonus (the server's speed: RunSpeed 5,
//! then MovementBonus on top, gameplay/player_combat_stats.py).
//! UNVERIFIED: the rider's attachment is taken as the `mount` joint's transform
//!   (position and rotation) in the mount's frame; the client code that seats
//!   the rider was not traced.

use std::collections::HashMap;

use bevy::asset::{io::Reader, AssetLoader, LoadContext};
use bevy::prelude::*;
use serde::Deserialize;

use crate::actors::character::{AnimState, Character, CharacterAnim, ModelBones};
use crate::actors::monsters::{json_loader, spawn_model};
use crate::gameplay::skills::{SkillData, StatusTable};
use crate::network::net::{LocalPlayer, Net, RemotePlayer, RUN_SPEED};

#[derive(Deserialize, Clone, Debug)]
pub struct MountDef {
    pub graphics: String,
    pub set: String,
    pub anim_set: String,
    /// The speed its run clip was made for (AnimRunSpeed).
    pub run_speed: f32,
    pub dye: [[f32; 4]; 2],
}

#[derive(Asset, TypePath, Deserialize)]
#[serde(transparent)]
pub struct MountTable(pub HashMap<String, MountDef>);
json_loader!(MountTableLoader, MountTable, "mounts.json");

#[derive(Resource)]
struct Data(Handle<MountTable>);

/// On a holder while its ride effect lasts.
#[derive(Component)]
pub struct Riding {
    instance: u32,
    /// Its MovementBonus (crate::network::net::walk_local adds it).
    pub bonus: f32,
    model: Entity,
    run_speed: f32,
    /// How high the saddle seats the rider over the ground (seat_riders), for
    /// the name over them (crate::actors::nameplate).
    pub lift: f32,
}

/// A mount model, following its rider (follow_riders).
#[derive(Component)]
struct MountOf(Entity);

/// Ride events not applied yet: their holder or the tables not in.
#[derive(Default)]
struct Pending(Vec<(u32, u16, u32, bool, f32)>);

pub struct MountsPlugin;

impl Plugin for MountsPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<MountTable>()
            .register_asset_loader(MountTableLoader)
            .add_systems(Startup, |mut commands: Commands, assets: Res<AssetServer>| {
                commands.insert_resource(Data(assets.load("characters/mounts.json")));
            })
            .add_systems(Update, apply_rides)
            .add_systems(PostUpdate, follow_riders.before(TransformSystems::Propagate))
            .add_systems(PostUpdate, seat_riders.after(TransformSystems::Propagate));
    }
}

#[allow(clippy::too_many_arguments)]
/// A player wearing a status effect's transformation (StatusDef::transform),
/// by that effect's instance.
#[derive(Component)]
pub(crate) struct Transformed {
    instance: u32,
}

fn apply_rides(
    mut commands: Commands,
    mut net: Option<NonSendMut<Net>>,
    skills: Option<Res<SkillData>>,
    statuses: Res<Assets<StatusTable>>,
    data: Res<Data>,
    mounts: Res<Assets<MountTable>>,
    assets: Res<AssetServer>,
    time: Res<Time>,
    actors: Query<(Entity, Option<&LocalPlayer>, Option<&RemotePlayer>), With<Character>>,
    mut riders: Query<(&mut Character, Option<&Riding>, Option<&Transformed>)>,
    mut pending: Local<Pending>,
    (library, variations): (Option<Res<crate::actors::character::CharacterLibrary>>, Res<Assets<crate::actors::npc::Variations>>),
    mut tested: Local<(u8, f32)>,
    mut redraw: Local<Vec<(Entity, crate::network::net::StatusEvent)>>,
    mut worn: Local<HashMap<Entity, u32>>,
) {
    let now = time.elapsed_secs();
    if let Some(net) = net.as_mut() {
        // DSOR_TEST_MOUNT=<ride effect id>: the offline player gets the server's
        // sequence (customization.py): the cast effect (cast_*, 2.4 s), then the
        // ride, then the cast's removal (screenshots).
        if net.offline && tested.0 < 2 {
            if let (Ok(ride), Some(table)) = (std::env::var("DSOR_TEST_MOUNT"), skills.as_ref().and_then(|s| statuses.get(s.status()))) {
                let find = |id: &str| table.0.iter().find(|(_, d)| d.id == id).map(|(i, _)| *i as u16);
                let cast = ride.replacen("ride_", "cast_", 1);
                if let (Some(ride_index), Some(local)) = (find(&ride), net.local_actor) {
                    let event = |index: u16, instance: u32, seconds: f32| crate::network::net::StatusEvent { holder: local, index, instance, seconds, removed: false };
                    if tested.0 == 0 && now > 3.0 {
                        tested.0 = 1;
                        tested.1 = now;
                        if let Some(c) = find(&cast) {
                            net.mount_events.push((local, c, 1, true));
                            net.status_events.push(event(c, 1, 3.5));
                        }
                    } else if tested.0 == 1 && now - tested.1 >= 3.5 {
                        tested.0 = 2;
                        net.mount_events.push((local, ride_index, 2, true));
                        net.status_events.push(event(ride_index, 2, 24840.0));
                        net.stat_effects.insert(2, dsor_proto::commands::combat::StatusEffectElement {
                            index: ride_index, instance: 2,
                            tail: Some(Default::default()), ..Default::default()
                        });
                        net.stat_revision = net.stat_revision.wrapping_add(1);
                        if let Some(c) = find(&cast) {
                            net.mount_events.push((local, c, 1, false));
                            net.status_events.push(crate::network::net::StatusEvent { removed: true, ..event(c, 1, 3.5) });
                        }
                    }
                }
            }
        }
        pending.0.extend(std::mem::take(&mut net.mount_events).into_iter().map(|(h, i, n, a)| (h, i, n, a, now)));
    }
    if !redraw.is_empty() {
        if let Some(net) = net.as_mut() {
            redraw.retain(|(entity, event)| match riders.get(*entity) {
                Ok((character, _, transformed)) if transformed.is_some_and(|t| t.instance == event.instance) => {
                    if character.outfit_ready() && character.skeleton_root().is_some() {
                        net.status_events.push(event.clone());
                        false
                    } else {
                        true
                    }
                }
                _ => false,
            });
        }
    }
    if pending.0.is_empty() {
        return;
    }
    let (Some(table), Some(mounts)) = (skills.as_ref().and_then(|s| statuses.get(s.status())), mounts.get(&data.0)) else { return };
    let mut keep = Vec::new();
    for (holder, index, instance, added, at) in std::mem::take(&mut pending.0) {
        let Some(def) = table.0.get(&(index as u32)).filter(|d| !d.mount.is_empty() || d.transform.is_some()) else { continue };
        let entity = actors.iter().find_map(|(e, l, r)| {
            (l.is_some_and(|l| l.actor == holder) || r.is_some_and(|r| r.actor == holder)).then_some(e)
        });
        let Some(entity) = entity else {
            // The holder may not be spawned yet (a ride already on at arrival).
            if now - at < 10.0 {
                keep.push((holder, index, instance, added, at));
            }
            continue;
        };
        let Ok((mut character, riding, transformed)) = riders.get_mut(entity) else { continue };
        if let Some(t) = &def.transform {
            if added {
                let skeleton = character.desc.skeleton().to_owned();
                let variation = library.as_deref().map(|l| crate::actors::character::named_variation(l, &variations, &skeleton, &t.variation)).unwrap_or_default();
                if variation.is_empty() && !t.variation.is_empty() && now - at < 10.0 {
                    // Its skeleton's shapes are still loading.
                    keep.push((holder, index, instance, added, at));
                    continue;
                }
                let look = crate::actors::character::NpcLook {
                    sound_profile: String::new(),
                    skeleton,
                    parts: vec![t.skin.clone()],
                    anim_set: t.anim_set.clone(),
                    variation,
                };
                crate::actors::character::reshape(&mut commands, &mut character, Some(look));
                commands.entity(entity).try_insert(Transformed { instance });
                worn.insert(entity, instance);
                // Its own visual (dwarf_irondwarf_idle on fx_chest) rode the old
                // skeleton's joints, gone with it: drawn again on the new one.
                // FAILURE: "y'a plus de FX de transformation".
                redraw.push((entity, crate::network::net::StatusEvent { holder, index, instance, seconds: 3600.0, removed: false }));
                if let Some(net) = net.as_mut().filter(|n| n.local_actor == Some(holder) && !t.skills.is_empty()) {
                    let base = net.quickbars.bars.first().cloned().unwrap_or_default();
                    net.hidden_bar = Some(crate::network::net::hidden_bar(&base, &t.skills));
                }
                info!("{holder:#x} transforms ({})", def.id);
            } else if worn.get(&entity).copied().or(transformed.map(|t| t.instance)) == Some(instance) {
                // CONTRACT: the instance worn now, as this loop left it: a
                //   costume swap sends the new one, then the old one's removal,
                //   in one batch, before Transformed is updated.
                // FAILURE: "quand j'ai un costume et que j'en equipe un autre ca
                //   me remet ma tenue initiale".
                worn.remove(&entity);
                crate::actors::character::reshape(&mut commands, &mut character, None);
                commands.entity(entity).remove::<Transformed>();
                if let Some(net) = net.as_mut().filter(|n| n.local_actor == Some(holder)) {
                    net.hidden_bar = None;
                }
                info!("{holder:#x} transforms back ({})", def.id);
            }
            continue;
        }
        if added {
            if riding.is_some_and(|r| r.instance == instance) {
                continue;
            }
            if let Some(old) = riding {
                commands.entity(old.model).try_despawn();
            }
            let Some(mount) = mounts.0.get(&def.mount) else {
                warn!("ride effect {}: no mount {}", def.id, def.mount);
                continue;
            };
            let model = spawn_model(&mut commands, &assets, &mount.graphics, &mount.set, &mount.anim_set, mount.dye);
            // CONTRACT: not a child of its rider. crate::actors::character binds
            //   every skinned mesh under a character to the character's own
            //   skeleton (keep_parts_bound): under the rider, the mount's skin was
            //   rebound to dead joints and drawn nowhere, or where they had been.
            // FAILURE (2026-10-16): "les montures sont invisibles", "le tigre est
            //   coince a un point de la map".
            commands.entity(model).insert((Name::new(format!("mount {}", def.mount)), MountOf(entity)));
            commands.entity(entity).try_insert(Riding { instance, bonus: def.movement_bonus, model, run_speed: mount.run_speed, lift: 0.0 });
            character.riding = Some(mount.anim_set.clone());
            info!("{holder:#x} rides {} (+{} speed)", def.mount, def.movement_bonus);
        } else if let Some(r) = riding.filter(|r| r.instance == instance) {
            commands.entity(r.model).try_despawn();
            commands.entity(entity).remove::<Riding>();
            character.riding = None;
            if let Some(skeleton) = character.skeleton_root() {
                commands.entity(skeleton).try_insert(Transform::IDENTITY);
            }
            info!("{holder:#x} gets off {}", def.mount);
        }
    }
    pending.0 = keep;
}

/// Each mount where its rider stands, facing its way; gone with its rider.
fn follow_riders(
    mut commands: Commands,
    riders: Query<(&Transform, Option<&ChildOf>, Option<&Riding>), Without<MountOf>>,
    parents: Query<&GlobalTransform>,
    mut mounts: Query<(Entity, &MountOf, &mut Transform)>,
) {
    for (e, of, mut tf) in &mut mounts {
        let Ok((rider, parent, riding)) = riders.get(of.0) else {
            commands.entity(e).try_despawn();
            continue;
        };
        if riding.is_none_or(|r| r.model != e) {
            commands.entity(e).try_despawn();
            continue;
        }
        let at = match parent.and_then(|p| parents.get(p.parent()).ok()) {
            Some(p) => Transform::from_matrix((p.affine() * rider.to_matrix()).into()),
            None => *rider,
        };
        let at = at.with_scale(Vec3::ONE);
        if *tf != at {
            *tf = at;
        }
    }
}

/// The rider on the saddle, the mount running or standing with it.
fn seat_riders(
    mut riders: Query<(&GlobalTransform, &Character, &mut CharacterAnim, &mut Riding), Without<ModelBones>>,
    mut mounts: Query<(&ModelBones, &mut CharacterAnim), Without<Riding>>,
    globals: Query<&GlobalTransform>,
    mut transforms: Query<&mut Transform>,
    mut players: Query<&mut AnimationPlayer>,
) {
    for (root, character, mut anim, mut riding) in &mut riders {
        let Ok((bones, mut mount_anim)) = mounts.get_mut(riding.model) else { continue };
        let running = anim.state == AnimState::Run;
        let state = if running { AnimState::Run } else { AnimState::Idle };
        if mount_anim.state != state {
            mount_anim.state = state;
        }
        // Its run clip at the speed it covers ground (AnimRunSpeed).
        let speed = if running && riding.run_speed > 0.0 { (RUN_SPEED + riding.bonus) / riding.run_speed } else { 1.0 };
        if (mount_anim.speed - speed).abs() > 1e-3 {
            mount_anim.speed = speed;
        }
        // The rider's clip is the mount's twin (mount_run_horse_01 under the
        // rider): same rate, same phase. The rider kept its own run rate
        // (movement speed, set only on foot) and its own start.
        // FAILURE: "un bug de sync entre l'animation du personnage et de sa monture".
        if (anim.speed - speed).abs() > 1e-3 {
            anim.speed = speed;
        }
        if let (Some((mount_player, mount_node, mount_len)), Some((rider_player, rider_node, rider_len))) = (mount_anim.playing(), anim.playing()) {
            let phase = players.get(mount_player).ok().and_then(|p| p.animation(mount_node)).map(|a| a.seek_time());
            if let (Some(phase), true) = (phase, mount_len > 0.0 && rider_len > 0.0) {
                let wanted = (phase / mount_len).fract() * rider_len;
                if let Some(active) = players.get_mut(rider_player).ok().as_mut().and_then(|p| p.animation_mut(rider_node)) {
                    let drift = (active.seek_time() - wanted).abs();
                    if drift > 0.05 && drift < rider_len - 0.05 { active.seek_to(wanted); }
                }
            }
        }
        // Every mount skeleton has a `mount` joint (polarbear, horse, mount_tiger...);
        // only some also have a Saddle. FAILURE (2026-10-16): seated on Saddle
        // alone, a tiger or horse rider stayed at the root ("je suis dans le sol").
        let seat = bones.0.get("mount").or_else(|| bones.0.get("Saddle")).and_then(|s| globals.get(*s).ok());
        let (Some(saddle), Some(skeleton)) = (seat, character.skeleton_root()) else { continue };
        let local = root.affine().inverse() * saddle.affine();
        if (riding.lift - local.translation.y).abs() > 1e-3 { riding.lift = local.translation.y; }
        if let Ok(mut tf) = transforms.get_mut(skeleton) {
            let seat = Transform::from_matrix(local.into());
            if *tf != seat {
                *tf = seat;
            }
        }
    }
}
