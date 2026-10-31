//! Pets: a pet status effect puts its companion at its holder's side.
//!
//! The server only runs the effect (DSOR_Server services/customization.py
//! `_toggle_switchable_sticker`: using a pet from the bag or the sticker book
//! starts its persistent status effect and ends the previous pet's; the local
//! initialization sends the persistent effects, presence_visuals.py
//! `_remote_status_snapshot` a player coming into view). The effect's
//! StartModifiers name the pet ("Pet:PetMonster,pet_frogman#~"); the client makes
//! it, from _Template_PetMonster (tools/export_pets.py: characters/pets.json).
//!
//! As dro_client64.exe does it (client/properties/pet*.cc, addresses of the 2026
//! build in D:\_dso_retired\dso-server\.reverse-current):
//! EVIDENCE: PetProperty::CreatePet 0x1401840d0 puts the pet at
//!   CalculateTargetPosition 0x140183dac -- 2.3 from its master, diagonally behind
//!   and to its left ((-1, 0, 1) in the master's frame, -Z ahead), 4.3 while the
//!   master rides (Messaging::Mount / Dismount, ClientActorObserver::OnMountApplied
//!   0x1401df750) -- turned as its master, its collision off.
//! EVIDENCE: PetMovementProperty::OnBeginFrame 0x140183064: farther than 20 from
//!   its master it is put back at that place (0x1401834a4); standing, it walks to
//!   its master once farther than 2.4 (4.4 riding) and stops 1.7 (4.2) from it
//!   (MoveGoto, 0x140182e00); having stood 30 s (the constructor's first wait,
//!   0x140182890), then a random 30-60 s each time, it plays its
//!   AlternativeIdleSequence (PetSequenceProperty 0x140185768).
//! EVIDENCE: the master forwards its relative velocity to its pet (MoveSetVelocity,
//!   PetProperty 0x140184890): the pet runs its RunSpeed times its master's factor.
//! EVIDENCE: PetSequenceProperty: its SummonSequence and PermanentSequence when it
//!   appears (0x140185f9c), its DeathSequence when its effect ends, ending the
//!   others (0x140185b84); every one follows it (0x1401859d4). PetProperty keeps a
//!   removed pet DespawnTime seconds more (RemovedPet, 0x140184e8c, 0x1401851d8),
//!   its movement still following the master. A master that goes (out of view,
//!   another map) takes its pet at once, a removed one too (OnDeactivate
//!   0x140184cd4).
//! EVIDENCE: a ReviveCommand with a cooldown (handler 0x1401cb46c) sends the
//!   revived actor ActivateAlternativeDespawnSequence(true), which its
//!   PetProperty forwards to the pet: the pet then leaves with its
//!   AlternativeDeathSequence (0x140185b84; the phoenixes' "revive", the pets
//!   whose effect says "respawn"). The live server sends every revive with
//!   cooldown 0, so this never happens online yet.
//! Pets are scenery: no Monster, Npc or RemotePlayer, so nothing hovers,
//!   highlights, targets or strikes them.
//! UNVERIFIED: how often PathEngine re-paths a MoveGoto after a moving target:
//!   PathEngineActorProperty::MoveGotoTarget 0x1406f85f8 hands the master's
//!   PathEngine agent and the stop distance to the library. Here the pet walks
//!   straight on the navigation mesh and, where that is blocked, along
//!   NavMesh::path, looked for again every 0.5 s.

use std::collections::HashMap;

use bevy::asset::{io::Reader, AssetLoader, LoadContext};
use bevy::prelude::*;
use serde::Deserialize;

use crate::actors::character::{AnimState, CharacterAnim};
use crate::actors::monsters::{json_loader, spawn_model, Monster};
use crate::actors::mounts::Riding;
use crate::gameplay::skills::{play_attached, play_with, Sequence, SequenceHidden, SequencePlayer, SequenceTable, SkillData, Track};
use crate::network::net::{turn_towards, LocalPlayer, Net, RemotePlayer, RUN_SPEED};
use crate::world::nav::{CurrentNav, NavMesh};

#[cfg(test)]
mod tests;

/// _Template_PetMonster (tools/export_pets.py).
#[derive(Deserialize, Clone, Debug)]
pub struct PetDef {
    pub graphics: String,
    pub set: String,
    pub anim_set: String,
    /// RunSpeed, and the speed its run clip was made for (AnimRunSpeed).
    pub run_speed: f32,
    pub anim_run_speed: f32,
    pub dye: [[f32; 4]; 2],
    /// Its sequences, by bare name (skills/sequences.json).
    #[serde(default)]
    pub summon: String,
    #[serde(default)]
    pub death: String,
    /// AlternativeDeathSequence: its despawn once it revived its master (the
    /// phoenixes' "revive").
    #[serde(default)]
    pub alt_death: String,
    #[serde(default)]
    pub idle_alt: String,
    #[serde(default)]
    pub permanent: String,
    /// DespawnTime: seconds it stays once its effect ends.
    #[serde(default)]
    pub despawn_s: f32,
    /// Its anim set's Spawn clip, for a SummonSequence without an animation.
    #[serde(default)]
    pub spawn_s: f32,
}

#[derive(Asset, TypePath, Deserialize, Debug)]
pub struct PetTable {
    /// Status effect wire index -> the pet its StartModifiers name.
    pub effects: HashMap<u32, String>,
    pub pets: HashMap<String, PetDef>,
}
json_loader!(PetTableLoader, PetTable, "pets.json");

#[derive(Resource)]
struct Data(Handle<PetTable>);

/// Where the pet stands from its master, diagonally behind and to the left
/// (CalculateTargetPosition), riding or not.
const PLACE: [f32; 2] = [2.3, 4.3];
/// It walks once farther than this from its master...
const FOLLOW: [f32; 2] = [2.4, 4.4];
/// ...and stops this close (MoveGoto's distance).
const STOP: [f32; 2] = [1.7, 4.2];
/// Farther than this, it is put back at its place.
const TELEPORT: f32 = 20.0;
/// Standing before its first alternative idle, then a random wait in between.
const FIRST_IDLE_WAIT: f32 = 30.0;
const IDLE_WAIT: [f32; 2] = [30.0, 60.0];
/// How often a walking pet looks for a way around what blocks it.
const REROUTE_S: f32 = 0.5;
/// The highest step a pet climbs in one move (the player's, crate::network::net).
const MAX_STEP: f32 = 1.2;
const FRAMES_PER_SECOND: f32 = 25.0;

/// On a master while its pet effect lasts.
#[derive(Component, Clone, Copy, Debug)]
pub struct HasPet {
    instance: u32,
    pub pet: Entity,
}

/// A pet, following its master.
#[derive(Component)]
pub struct Pet {
    master: Entity,
    template: String,
    /// Its summon has played (once its model is in; hidden until then).
    summoned: bool,
    /// Walking to its master (a MoveGoto not finished).
    moving: bool,
    /// When it last stopped or appeared, and how long it then stands before its
    /// alternative idle (PetMovementProperty +0x70, +0x78).
    idle_since: f32,
    idle_wait: f32,
    /// Its summon's Spawn animation, or its despawn's, plays until then: no run
    /// or idle over it (the sequencer hijacks the animation; then the pet's own
    /// runs on, PetSequenceProperty::SetupSequencer 0x140186650 turning the
    /// track's idle restart off).
    animating_until: f32,
    /// Its SummonSequence, PermanentSequence and AlternativeIdleSequence players,
    /// ended when it leaves (PetSequenceProperty +0x60, +0x78, +0x70).
    sequences: [Option<Entity>; 3],
    /// Waypoints around what blocks the straight way to its master, and when they
    /// were looked for.
    route: Vec<Vec3>,
    routed_at: f32,
    /// It revived its master (ActivateAlternativeDespawnSequence): it leaves
    /// with its AlternativeDeathSequence (PetSequenceProperty +0x80).
    revived: bool,
}

/// A pet whose effect ended: its DeathSequence plays while it still follows its
/// master (its PetMovementProperty lives on), then it goes.
#[derive(Component)]
struct Leaving {
    until: f32,
}

/// A StatusEffect element not applied yet: its master or the table not in.
struct PetEvent {
    holder: u32,
    index: u16,
    instance: u32,
    added: bool,
    at: f32,
}

pub struct PetsPlugin;

impl Plugin for PetsPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<PetTable>()
            .register_asset_loader(PetTableLoader)
            .add_systems(Startup, |mut commands: Commands, assets: Res<AssetServer>| {
                commands.insert_resource(Data(assets.load("characters/pets.json")));
            })
            // After the masters moved; before the sequences, so a summon's own
            // hide track takes the pet the frame it appears.
            .add_systems(Update, (test_pet, apply_pets, follow_masters).chain()
                .after(crate::network::net::NetSystems)
                .before(crate::gameplay::skills::play_sequences));
    }
}

/// Where a pet stands from its master (CalculateTargetPosition 0x140183dac): the
/// master's frame has -Z ahead (crate::gameplay::skills::entity_rotation), the
/// actor's own +Z.
pub(crate) fn place(master: &Transform, riding: bool) -> Vec3 {
    let d = PLACE[riding as usize] * std::f32::consts::FRAC_1_SQRT_2;
    master.translation + master.rotation * Vec3::new(d, 0.0, -d)
}

/// What a pet does this frame (PetMovementProperty::OnBeginFrame 0x140183064).
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Follow {
    /// Too far: put back at its place.
    Teleport,
    /// Walk on toward its master.
    Walk,
    /// Arrived: stop.
    Stop,
    /// Stand where it is.
    Stand,
}

pub(crate) fn follow(distance: f32, moving: bool, riding: bool) -> Follow {
    let r = riding as usize;
    if distance > TELEPORT {
        Follow::Teleport
    } else if moving {
        if distance <= STOP[r] { Follow::Stop } else { Follow::Walk }
    } else if distance > FOLLOW[r] {
        Follow::Walk
    } else {
        Follow::Stand
    }
}

/// A number in [0, 1) from a seed (splitmix64): the alternative idle's wait.
fn roll(seed: u64) -> f32 {
    let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 40) as f32 / (1u64 << 24) as f32
}

/// The ground under a pet's place, or the place itself without a mesh.
fn on_ground(mesh: Option<&NavMesh>, at: Vec3) -> Vec3 {
    let Some(mesh) = mesh else { return at };
    mesh.ground(at.x, at.z, at.y, MAX_STEP).map(|y| Vec3::new(at.x, y, at.z))
        .or_else(|| mesh.nearest(at, 3.0))
        .unwrap_or(at)
}

/// How fast its master runs, for the pet's factor (crate::network::net::walk_local's
/// speed for the player; a remote one's records, at least its riding speed).
fn master_speed(local: bool, remote: Option<&RemotePlayer>, riding: Option<&Riding>, stats: Option<&crate::gameplay::stats::CharacterStats>) -> f32 {
    let base = RUN_SPEED + riding.map_or(0.0, |r| r.bonus);
    if local {
        stats.and_then(|s| s.movement_speed()).unwrap_or(base)
    } else {
        remote.map_or(0.0, |r| r.speed).min(base * 2.0).max(base)
    }
}

/// A pet's SummonSequence; with its anim set's Spawn clip when the sequence has
/// no animation of its own (StartAnimationState "Spawn;Idle").
fn summon_sequence(def: &PetDef, sequences: Option<&SequenceTable>) -> Option<Sequence> {
    let mut seq = sequences.and_then(|s| s.0.get(&def.summon)).cloned();
    if def.spawn_s > 0.0 && !seq.as_ref().is_some_and(|s| s.tracks.iter().any(|t| matches!(t, Track::Anim { .. }))) {
        let end = (def.spawn_s * FRAMES_PER_SECOND).ceil() as i32;
        let seq = seq.get_or_insert_with(|| Sequence { length: 0, repeat: false, tracks: Vec::new() });
        seq.tracks.push(Track::Anim { name: "Spawn".into(), speed: 1.0, start: 0, end, fadein: 0.0 });
        seq.length = seq.length.max(end as u32);
    }
    seq
}

#[allow(clippy::too_many_arguments)]
fn apply_pets(
    mut commands: Commands,
    net: Option<NonSendMut<Net>>,
    data: Res<Data>,
    tables: Res<Assets<PetTable>>,
    (skills, sequences): (Option<Res<SkillData>>, Res<Assets<SequenceTable>>),
    assets: Res<AssetServer>,
    time: Res<Time>,
    (nav, meshes): (Option<Res<CurrentNav>>, Res<Assets<NavMesh>>),
    masters: Query<(Entity, &Transform, Option<&LocalPlayer>, Option<&RemotePlayer>, Option<&HasPet>, Has<Riding>),
        (Or<(With<LocalPlayer>, With<RemotePlayer>)>, Without<Monster>, Without<Pet>)>,
    mut pets: Query<&mut Pet>,
    mut players: Query<&mut SequencePlayer>,
    mut pending: Local<Vec<PetEvent>>,
    mut warned: Local<bool>,
) {
    let now = time.elapsed_secs();
    let table = tables.get(&data.0);
    // Taken only when there are some: a write marks Net changed.
    if let Some(mut net) = net.filter(|n| !n.pet_events.is_empty() || !n.pet_revivals.is_empty()) {
        pending.extend(std::mem::take(&mut net.pet_events).into_iter()
            .map(|(holder, index, instance, added)| PetEvent { holder, index, instance, added, at: now }));
        // Revived by its pet: the pet out now leaves with its alternative
        // despawn, whenever its effect ends (PetProperty forwards
        // ActivateAlternativeDespawnSequence to the pet it has, 0x140184890).
        for actor in std::mem::take(&mut net.pet_revivals) {
            let has = masters.iter().find(|(_, _, l, r, _, _)| {
                l.is_some_and(|l| l.actor == actor) || r.is_some_and(|r| r.actor == actor)
            }).and_then(|(.., has, _)| has);
            if let Some(Ok(mut pet)) = has.map(|h| pets.get_mut(h.pet)) {
                pet.revived = true;
            }
        }
    }
    if pending.is_empty() {
        return;
    }
    let Some(table) = table else {
        // Every status effect comes here: kept while the table loads, not for a
        // session without it (assets exported before tools/export_pets.py).
        pending.retain(|e| now - e.at < 10.0);
        if !*warned && assets.load_state(&data.0).is_failed() {
            *warned = true;
            warn!("characters/pets.json did not load: no pets (run tools/export_pets.py)");
        }
        return;
    };
    let mesh = nav.as_ref().and_then(|n| meshes.get(&n.0));
    let sequences = skills.as_ref().and_then(|s| sequences.get(s.sequences()));
    // A pet changed earlier in this batch (the server adds the new pet's effect
    // before it ends the old one's): this run's view of each master's pet.
    let mut changed: HashMap<Entity, Option<HasPet>> = HashMap::new();
    let mut keep = Vec::new();
    for event in std::mem::take(&mut *pending) {
        let Some(id) = table.effects.get(&(event.index as u32)) else { continue };
        let master = masters.iter().find(|(_, _, l, r, _, _)| {
            l.is_some_and(|l| l.actor == event.holder) || r.is_some_and(|r| r.actor == event.holder)
        });
        let Some((entity, at, _, _, has, riding)) = master else {
            // Its master may not be in yet (a pet already out on arrival).
            if now - event.at < 10.0 {
                keep.push(event);
            }
            continue;
        };
        let current = changed.get(&entity).copied().unwrap_or(has.copied());
        if event.added {
            if current.is_some_and(|c| c.instance == event.instance) {
                continue;
            }
            let Some(def) = table.pets.get(id) else {
                warn!("pet effect {}: no pet {id}", event.index);
                continue;
            };
            if let Some(old) = current {
                dismiss(&mut commands, old.pet, table, sequences, now, &mut pets, &mut players);
            }
            let model = spawn_model(&mut commands, &assets, &def.graphics, &def.set, &def.anim_set, def.dye);
            // CONTRACT: not a child of its master (crate::actors::mounts: a skinned
            //   mesh under a character is bound to the character's skeleton).
            let tf = Transform::from_translation(on_ground(mesh, place(at, riding))).with_rotation(at.rotation);
            commands.entity(model).insert((
                tf,
                Visibility::Hidden,
                Name::new(format!("pet {id}")),
                Pet { master: entity, template: id.clone(), summoned: false, moving: false, idle_since: now,
                    idle_wait: FIRST_IDLE_WAIT, animating_until: now, sequences: [None; 3], route: Vec::new(),
                    routed_at: f32::NEG_INFINITY, revived: false },
            ));
            let has = HasPet { instance: event.instance, pet: model };
            commands.entity(entity).try_insert(has);
            changed.insert(entity, Some(has));
            info!("{:#x} calls its pet {id}", event.holder);
        } else if let Some(c) = current.filter(|c| c.instance == event.instance) {
            dismiss(&mut commands, c.pet, table, sequences, now, &mut pets, &mut players);
            commands.entity(entity).remove::<HasPet>();
            changed.insert(entity, None);
            info!("{:#x} sends its pet {id} away", event.holder);
        }
    }
    *pending = keep;
}

/// The sequence a leaving pet plays (PetSequenceProperty 0x140185b84).
/// UNVERIFIED: revived without an AlternativeDeathSequence; the client's
///   "misc/" lookup then finds nothing (every pet whose effect can revive, the
///   "respawn" ones, has one): here the DeathSequence plays.
fn despawn_sequence(def: &PetDef, revived: bool) -> &str {
    if revived && !def.alt_death.is_empty() { &def.alt_death } else { &def.death }
}

/// Seconds of a sequence's animation tracks: the time it hijacks the pet's
/// animation.
fn animation_seconds(seq: &Sequence) -> f32 {
    let end = seq.tracks.iter().filter_map(|t| match t { Track::Anim { end, .. } => Some(*end), _ => None }).max().unwrap_or(0);
    end.max(0) as f32 / FRAMES_PER_SECOND
}

/// A pet whose effect ended: its other sequences end (DiscardSequencer), its
/// DeathSequence plays (its AlternativeDeathSequence once it revived its
/// master), and it goes after its DespawnTime.
#[allow(clippy::too_many_arguments)]
fn dismiss(commands: &mut Commands, pet: Entity, table: &PetTable, sequences: Option<&SequenceTable>, now: f32,
    pets: &mut Query<&mut Pet>, players: &mut Query<&mut SequencePlayer>) {
    let Ok(mut p) = pets.get_mut(pet) else {
        commands.entity(pet).try_despawn();
        return;
    };
    for player in p.sequences.iter_mut().filter_map(Option::take) {
        if let Ok(mut player) = players.get_mut(player) {
            player.stop = true;
        }
    }
    let def = table.pets.get(&p.template);
    let lasts = if p.summoned { def.map_or(0.0, |d| d.despawn_s) } else { 0.0 };
    let name = def.map(|d| despawn_sequence(d, p.revived));
    if let Some((name, seq)) = name.and_then(|n| sequences.and_then(|s| s.0.get(n)).map(|s| (n, s))).filter(|_| p.summoned) {
        info!("pet {} leaves ({name})", p.template);
        p.animating_until = now + animation_seconds(seq);
        // Hidden by it, it stays hidden until it is gone: DespawnTime is authored
        // to the sequence's end (the dragon's 51 frames, 2.04 s), and a hide
        // track ending first would show it again for a frame.
        let mut seq = seq.clone();
        let gone = (lasts * FRAMES_PER_SECOND).ceil() as i32 + FRAMES_PER_SECOND as i32;
        for track in &mut seq.tracks {
            if let Track::Hide { end, .. } = track {
                *end = (*end).max(gone);
                seq.length = seq.length.max(gone as u32);
            }
        }
        play_attached(commands, seq, pet);
    }
    commands.entity(pet).try_insert(Leaving { until: now + lasts });
}

#[allow(clippy::too_many_arguments)]
fn follow_masters(
    mut commands: Commands,
    time: Res<Time>,
    data: Res<Data>,
    tables: Res<Assets<PetTable>>,
    (skills, sequences): (Option<Res<SkillData>>, Res<Assets<SequenceTable>>),
    (nav, meshes): (Option<Res<CurrentNav>>, Res<Assets<NavMesh>>),
    stats: Option<Res<crate::gameplay::stats::CharacterStats>>,
    masters: Query<(&Transform, &Visibility, Has<LocalPlayer>, Option<&RemotePlayer>, Option<&Riding>), Without<Pet>>,
    mut pets: Query<(Entity, &mut Pet, &mut Transform, &mut Visibility, &mut CharacterAnim, Has<SequenceHidden>, Option<&Leaving>)>,
) {
    if pets.is_empty() {
        return;
    }
    let now = time.elapsed_secs();
    let dt = time.delta_secs();
    let table = tables.get(&data.0);
    let mesh = nav.as_ref().and_then(|n| meshes.get(&n.0));
    let sequences = skills.as_ref().and_then(|s| sequences.get(s.sequences()));
    for (e, mut pet, mut tf, mut visibility, mut anim, hidden, leaving) in &mut pets {
        let Ok((master, shown, local, remote, riding)) = masters.get(pet.master) else {
            // Its master left the view or the map: it goes at once, a leaving
            // one too (PetProperty::OnDeactivate 0x140184cd4).
            commands.entity(e).try_despawn();
            continue;
        };
        if leaving.is_some_and(|l| now >= l.until) {
            commands.entity(e).try_despawn();
            continue;
        }
        let def = table.and_then(|t| t.pets.get(&pet.template));
        // Shown with its master (SetVisibleMsg is forwarded to the pet), once in;
        // its own sequences' hide tracks have the last word.
        let wanted = if !pet.summoned || *shown == Visibility::Hidden { Visibility::Hidden } else { Visibility::Inherited };
        if !hidden && *visibility != wanted {
            *visibility = wanted;
        }
        if !pet.summoned {
            // Its summon plays once the model can show it: started while it
            // loads, the rise was over before the pet was there.
            if !anim.is_ready() {
                continue;
            }
            pet.summoned = true;
            pet.idle_since = now;
            if let Some(def) = def {
                if let Some(seq) = summon_sequence(def, sequences) {
                    pet.animating_until = now + animation_seconds(&seq);
                    info!("pet {} appears ({}, its spawn animation {:.2} s)", pet.template, def.summon, pet.animating_until - now);
                    pet.sequences[0] = Some(play_attached(&mut commands, seq, e));
                }
                if let Some(seq) = sequences.and_then(|s| s.0.get(&def.permanent)) {
                    pet.sequences[1] = Some(play_with(&mut commands, seq.clone(), Some(e), e, seq.repeat, true));
                }
            }
            if !hidden {
                *visibility = if *shown == Visibility::Hidden { Visibility::Hidden } else { Visibility::Inherited };
            }
        }
        let riding_now = riding.is_some();
        let distance = master.translation.distance(tf.translation);
        let mut run_rate = None;
        match follow(distance, pet.moving, riding_now) {
            Follow::Teleport => {
                tf.translation = on_ground(mesh, place(master, riding_now));
                tf.rotation = master.rotation;
                pet.moving = false;
                pet.idle_since = now;
            }
            Follow::Stop => {
                pet.moving = false;
                pet.idle_since = now;
            }
            Follow::Stand => {}
            Follow::Walk => {
                if !pet.moving {
                    pet.moving = true;
                    pet.routed_at = f32::NEG_INFINITY;
                }
                let speed = def.map_or(RUN_SPEED, |d| d.run_speed)
                    * master_speed(local, remote, riding, stats.as_deref()) / RUN_SPEED;
                if walk(&mut pet, &mut tf, master.translation, speed * dt, STOP[riding_now as usize], mesh, now, dt) {
                    // Arrived (the MoveGoto's end).
                    pet.moving = false;
                    pet.idle_since = now;
                }
                // Its run clip at the speed it covers ground (AnimRunSpeed).
                let clip = def.map_or(0.0, |d| d.anim_run_speed);
                run_rate = Some(if clip > 0.0 { speed / clip } else { 1.0 });
            }
        }
        // Its spawn or despawn plays out; then it runs while it walks, and keeps
        // a sequence's animation (its alternative idle) standing.
        if now >= pet.animating_until {
            let wanted = if pet.moving {
                AnimState::Run
            } else if matches!(anim.state, AnimState::Named(_)) {
                anim.state.clone()
            } else {
                AnimState::Idle
            };
            if anim.state != wanted {
                anim.state = wanted;
            }
            let rate = match anim.state {
                AnimState::Run => run_rate.unwrap_or(anim.speed),
                AnimState::Idle => 1.0,
                _ => anim.speed,
            };
            if (anim.speed - rate).abs() > 1e-3 {
                anim.speed = rate;
            }
            // Not while it leaves: its despawn holds the sequencer (0x140185768).
            if !pet.moving && leaving.is_none() && now > pet.idle_since + pet.idle_wait {
                pet.idle_since = now;
                pet.idle_wait = IDLE_WAIT[0] + (IDLE_WAIT[1] - IDLE_WAIT[0]) * roll(e.to_bits() ^ now.to_bits() as u64);
                if let Some(seq) = def.and_then(|d| sequences.and_then(|s| s.0.get(&d.idle_alt))) {
                    pet.sequences[2] = Some(play_attached(&mut commands, seq.clone(), e));
                }
            }
        }
    }
}

/// Offline captures of a pet coming and going.
#[derive(Default)]
struct TestPet {
    /// When the pet was called, and how many shots are taken.
    called: Option<f32>,
    dismissed: bool,
    shots: usize,
}

/// DSOR_TEST_PET=<pet>[,<seconds>]: offline, the player calls that pet 3 s after
/// the loading screen (time for DSOR_AUTOWALK), and sends it away that many
/// seconds later.
/// DSOR_SHOT_PET=<seconds>[:<seconds>...],<file>: a screenshot that long after the
/// call (`<file>.<i>.png`), then quit -- frame counts depend on the frame rate.
#[allow(clippy::too_many_arguments)]
fn test_pet(
    mut commands: Commands,
    net: Option<NonSendMut<Net>>,
    data: Res<Data>,
    tables: Res<Assets<PetTable>>,
    current: Option<Res<crate::world::map::CurrentMap>>,
    loading: Option<Res<crate::screens::loading::LoadingScreen>>,
    time: Res<Time>,
    mut state: Local<TestPet>,
    (mut loaded_at, mut enabled): (Local<Option<f32>>, Local<Option<bool>>),
) {
    if !*enabled.get_or_insert_with(|| std::env::var_os("DSOR_TEST_PET").is_some()) {
        return;
    }
    let Some(mut net) = net.filter(|n| n.offline) else { return };
    let Ok(spec) = std::env::var("DSOR_TEST_PET") else { return };
    let (Some(table), Some(local)) = (tables.get(&data.0), net.local_actor) else { return };
    let now = time.elapsed_secs();
    let Some(loaded) = *loaded_at else {
        // Once the loading screen is gone.
        if current.as_ref().is_some_and(|m| m.spawned) && loading.as_ref().is_some_and(|l| !l.active()) {
            *loaded_at = Some(now);
        }
        return;
    };
    let mut parts = spec.split(',');
    let pet = parts.next().unwrap_or_default().trim();
    let lasts = parts.next().and_then(|s| s.trim().parse::<f32>().ok());
    let Some(index) = table.effects.iter().filter(|(_, p)| p.as_str() == pet).map(|(i, _)| *i as u16).min() else {
        if state.called.is_none() {
            warn!("DSOR_TEST_PET: no effect puts out {pet}");
            state.called = Some(now);
            state.dismissed = true;
        }
        return;
    };
    let Some(called) = state.called else {
        if now >= loaded + 3.0 {
            state.called = Some(now);
            net.pet_events.push((local, index, 0x7000_0001, true));
        }
        return;
    };
    if !state.dismissed && lasts.is_some_and(|s| now >= called + s) {
        state.dismissed = true;
        net.pet_events.push((local, index, 0x7000_0001, false));
    }
    let Ok(shots) = std::env::var("DSOR_SHOT_PET") else { return };
    let Some((times, file)) = shots.split_once(',') else { return };
    let times: Vec<f32> = times.split(':').filter_map(|t| t.trim().parse().ok()).collect();
    if let Some(&at) = times.get(state.shots) {
        if now >= called + at {
            commands.spawn(bevy::render::view::screenshot::Screenshot::primary_window())
                .observe(bevy::render::view::screenshot::save_to_disk(format!("{file}.{}.png", state.shots)));
            info!("pet shot {} at {:.2} s after the call", state.shots, now - called);
            state.shots += 1;
        }
    } else if times.last().is_some_and(|last| now > called + last + 0.5) {
        commands.write_message(AppExit::Success);
    }
}

/// One step of a walking pet toward its master, on the ground, around what blocks
/// the straight way; true once it is `stop` from its master.
#[allow(clippy::too_many_arguments)]
fn walk(pet: &mut Pet, tf: &mut Transform, goal: Vec3, step: f32, stop: f32, mesh: Option<&NavMesh>, now: f32, dt: f32) -> bool {
    let here = tf.translation;
    if let Some(mesh) = mesh {
        if now - pet.routed_at >= REROUTE_S {
            pet.routed_at = now;
            pet.route = if mesh.walkable_segment(here, goal) { Vec::new() } else { mesh.path(here, goal).unwrap_or_default() };
        }
    }
    while pet.route.first().is_some_and(|w| Vec2::new(w.x - here.x, w.z - here.z).length() < 0.1) {
        pet.route.remove(0);
    }
    let waypoint = pet.route.first().copied().unwrap_or(goal);
    let mut to = waypoint - here;
    to.y = 0.0;
    let length = to.length();
    // Straight at its master, it stops short of it.
    let room = if pet.route.is_empty() { (length - stop).max(0.0) } else { length };
    if length < 1e-4 || (pet.route.is_empty() && room <= step) {
        if length >= 1e-4 {
            tf.translation = on_ground(mesh, here + to / length * room);
        }
        return pet.route.is_empty();
    }
    let dir = to / length;
    let wish = here + dir * step;
    let next = match mesh {
        None => Some(wish),
        Some(m) => {
            let on = |p: Vec3| m.ground(p.x, p.z, here.y, MAX_STEP).map(|h| Vec3::new(p.x, h, p.z));
            on(wish).or_else(|| on(Vec3::new(wish.x, here.y, here.z))).or_else(|| on(Vec3::new(here.x, here.y, wish.z)))
        }
    };
    if let Some(p) = next {
        tf.translation = p;
    }
    tf.rotation = turn_towards(tf.rotation, dir.x.atan2(dir.z), dt);
    false
}
