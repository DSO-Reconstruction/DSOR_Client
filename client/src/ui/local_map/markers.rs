//! Authored map symbols, with live faction colours and quest overrides.
use super::*;
use dsor_proto::commands::{actors::NpcInfo, ServerCommand};

#[derive(Component, Clone, Default)]
pub(crate) struct MapIcon(pub String);

/// A map id written under a source's symbol (an exit: where it leads), as the
/// map's caption (interface/maps.names.json).
#[derive(Component, Clone, Default)]
pub(crate) struct MapLabel(pub String);

#[derive(Clone, Copy, Default)]
struct Faction { id: u32, pvp: bool }

#[derive(Default)]
pub(crate) struct MapMarkers {
    local: Option<u32>,
    factions: HashMap<u32, Faction>,
    npcs: HashMap<u32, NpcInfo>,
    quests: HashMap<String, u8>,
}

impl MapMarkers {
    pub(crate) fn reset_map(&mut self) {
        self.local = None;
        self.factions.clear();
        self.npcs.clear();
    }

    pub(crate) fn receive(&mut self, command: &ServerCommand, actor: Option<u32>) {
        match command {
            ServerCommand::NewPlayer(p) => {
                self.local = actor;
                if let Some(a) = actor {
                    self.factions.insert(a, Faction { id: p.composite_words[0], pvp: p.composite_flags[0] });
                }
            }
            ServerCommand::NewRemotePlayer(p) => {
                if let Some(a) = actor { self.factions.insert(a, Faction { id: p.faction.faction_id, pvp: p.faction.pvp }); }
            }
            ServerCommand::RemotePlayerInfo(p) => {
                if let Some(a) = actor { self.factions.insert(a, Faction { id: p.faction.faction_id, pvp: p.faction.pvp }); }
            }
            ServerCommand::NewMonster(m) => {
                if let Some(a) = actor { self.factions.insert(a, Faction { id: m.faction.faction_id, pvp: m.faction.unknown_flag_0 }); }
            }
            ServerCommand::FactionInfo(f) => { self.factions.insert(f.owner, Faction { id: f.faction_id, pvp: f.pvp }); }
            ServerCommand::NpcInfo(n) => { if let Some(a) = actor { self.npcs.insert(a, n.clone()); } }
            ServerCommand::QuestLogInfo(log) => {
                self.quests = log.records.iter().map(|q| (q.id.to_lowercase(), q.state)).collect();
            }
            ServerCommand::QuestUpdate(q) if q.kind == 2 => {
                if let (Some((_, id)), Some((_, state))) = (q.strings.iter().find(|(k, _)| *k == 0), q.ints.iter().find(|(k, _)| *k == 2)) {
                    if let Ok(state) = u8::try_from(*state) { self.quests.insert(id.to_lowercase(), state); }
                }
            }
            ServerCommand::ActorsLeftVicinity(v) => {
                for a in &v.actors { self.factions.remove(a); self.npcs.remove(a); }
            }
            ServerCommand::DiscardMonster(_) => { if let Some(a) = actor { self.factions.remove(&a); } }
            _ => {}
        }
    }

    fn hostile(&self, actor: u32, monster: bool) -> bool {
        let local = self.local.and_then(|a| self.factions.get(&a));
        match (local, self.factions.get(&actor)) {
            (Some(local), Some(other)) => {
                if local.id == other.id { false }
                else { monster || (local.pvp && other.pvp) }
            }
            _ => monster,
        }
    }

    /// A quest's state in our log (QuestLogInfo / QuestUpdate): 2 closed.
    pub(crate) fn quest_state(&self, id: &str) -> Option<u8> { self.quests.get(&id.to_lowercase()).copied() }

    pub(crate) fn npc_style(&self, actor: u32, icon: &str) -> Option<Style> {
        let Some(info) = self.npcs.get(&actor) else { return Style::authored(icon) };
        if !info.enabled { return None; }
        // Turn-ins and active task targets take priority over new offers.
        let quest = info.interactions.iter().filter_map(|i| {
            let id = if i.quest.is_empty() { &i.name } else { &i.quest };
            let state = self.quests.get(&id.to_lowercase()).copied();
            let priority = match (i.kind, state) {
                (_, Some(2)) => return None, // closed
                (0, Some(1)) => 3, // ready to turn in
                (1..=3, _) => 2, // active encounter/talk/delivery target
                (0, None | Some(3)) => 1, // offered (or offered again after cancel)
                _ => return None,
            };
            Some((priority, i.state))
        }).max();
        if let Some((priority, kind)) = quest {
            let icon = if priority == 1 { "ui/icon_map_exclamationmark" } else { "ui/icon_map_questionmark" };
            let color = match kind {
                // PvP and repeatable PvP (Game::QuestInfo::Type 9, 3):
                // _Globals.QuestTypeColorPvP (0.76, 0.07, 0).
                // FAILURE: "je les vois en vert au lieu de rouge les quetes".
                3 | 9 => [194, 18, 0],
                0..=4 | 11..=13 => [40, 205, 255],
                7 => [200, 115, 255],
                8 => [255, 150, 45],
                _ => [255, 215, 65],
            };
            Some(Style { icon: icon.into(), color, size: 28 })
        } else { Style::authored(icon) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dsor_proto::commands::{actors::{NpcInteraction, QuestUpdate}, player::{NewPlayer, NewRemotePlayer}, social::FactionInfo};

    #[test]
    fn quest_updates_override_and_restore_the_authored_npc_icon() {
        let mut state = MapMarkers::default();
        let icon = "ui/icon_map_smith_03";
        let info = NpcInfo { enabled: true, interactions: vec![
            NpcInteraction { name: "quest".into(), quest: "Quest".into(), kind: 0, state: 6 },
        ], ..default() };
        state.receive(&ServerCommand::NpcInfo(info.clone()), Some(77));
        assert!(state.npc_style(77, icon).unwrap().icon.ends_with("exclamationmark"));
        let update = |value| ServerCommand::QuestUpdate(QuestUpdate {
            kind: 2, ints: vec![(2, value)], strings: vec![(0, "quest".into()), (6, "timestamp".into())],
        });
        state.receive(&update(1), None);
        assert!(state.npc_style(77, icon).unwrap().icon.ends_with("questionmark"));
        // A second offer must not cover the turn-in.
        let mut another = info.clone();
        another.interactions.push(NpcInteraction { name: "other".into(), kind: 0, state: 0, ..default() });
        state.receive(&ServerCommand::NpcInfo(another), Some(77));
        assert!(state.npc_style(77, icon).unwrap().icon.ends_with("questionmark"));
        state.receive(&ServerCommand::NpcInfo(info), Some(77));
        state.receive(&update(2), None);
        assert_eq!(state.npc_style(77, icon).unwrap().icon, icon);
        state.receive(&update(3), None);
        assert!(state.npc_style(77, icon).unwrap().icon.ends_with("exclamationmark"));
        state.receive(&ServerCommand::NpcInfo(NpcInfo { enabled: false, ..default() }), Some(77));
        assert!(state.npc_style(77, icon).is_none());
        state.reset_map();
        assert_eq!(state.npc_style(77, icon).unwrap().icon, icon);
    }

    #[test]
    fn faction_updates_change_player_dots_and_keep_team_mates_friendly() {
        let mut state = MapMarkers::default();
        state.receive(&ServerCommand::NewPlayer(NewPlayer { composite_words: [11, 1], composite_flags: [true, false], ..default() }), Some(1));
        state.receive(&ServerCommand::NewRemotePlayer(NewRemotePlayer::default()), Some(2));
        assert!(!state.hostile(2, false));
        let faction = |id, pvp| ServerCommand::FactionInfo(FactionInfo { owner: 2, faction_id: id, pvp, ..default() });
        state.receive(&faction(22, true), None);
        assert!(state.hostile(2, false));
        state.receive(&faction(11, true), None);
        assert!(!state.hostile(2, false));
        state.receive(&faction(22, false), None);
        assert!(!state.hostile(2, false));
        assert!(state.hostile(2, true));
    }

    #[test]
    fn marker_entities_follow_sources_and_disappear_with_them() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Image>().init_asset::<Mesh>().init_asset::<UiMaterial>().init_asset::<super::super::MapNames>()
            .add_systems(Update, update);
        let map = app.world_mut().spawn(MapGeometry {
            source: Handle::<NavMesh>::default().id(), view: View::new(MapMode::Compact, BASE_W),
            projection: GroundProjection::gameplay(), min: Vec2::ZERO, origin: Vec2::splat(20.0), scale: 2.0,
        }).id();
        app.world_mut().spawn((MarkerLayer, ChildOf(map)));
        let source = app.world_mut().spawn((MapIcon("ui/icon_map_amphora_gold".into()), GlobalTransform::from_translation(Vec3::X))).id();
        app.update();
        let mut query = app.world_mut().query::<(&Marker, &Transform)>();
        let (_, first) = query.single(app.world()).unwrap();
        let first = first.translation;
        app.world_mut().entity_mut(source).insert(GlobalTransform::from_translation(Vec3::Z * 10.0));
        app.update();
        let (marker, moved) = query.single(app.world()).unwrap();
        assert_eq!(marker.style.icon, "ui/icon_map_amphora_gold");
        assert_ne!(first, moved.translation);
        app.world_mut().despawn(source);
        app.update();
        assert_eq!(query.iter(app.world()).count(), 0);
    }

    #[test]
    fn overlapping_icons_keep_distinct_depths_through_style_changes_and_rebuilds() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Image>().init_asset::<Mesh>().init_asset::<UiMaterial>().init_asset::<super::super::MapNames>()
            .add_systems(Update, update);
        let map = app.world_mut().spawn(MapGeometry {
            source: Handle::<NavMesh>::default().id(), view: View::new(MapMode::Compact, BASE_W),
            projection: GroundProjection::gameplay(), min: Vec2::ZERO, origin: Vec2::ZERO, scale: 2.0,
        }).id();
        app.world_mut().spawn((MarkerLayer, ChildOf(map)));
        let sources: Vec<_> = (0..128).map(|_| app.world_mut().spawn((
            MapIcon("ui/icon_map_amphora".into()), GlobalTransform::IDENTITY,
        )).id()).collect();
        let snapshot = |world: &mut World| {
            let mut query = world.query::<(&Marker, &Transform)>();
            let mut rows: Vec<_> = query.iter(world).map(|(marker, at)| {
                assert_eq!(at.translation.xy(), Vec2::ZERO, "all icons overlap");
                assert!((2.0..3.0).contains(&at.translation.z), "icons stay below the player");
                // Include the window's depth to catch loss of float precision.
                (marker.target, 80.0 + at.translation.z)
            }).collect();
            rows.sort_by(|a, b| a.1.total_cmp(&b.1));
            assert!(rows.windows(2).all(|w| w[0].1 < w[1].1), "overlapping icons need unique sort keys");
            rows
        };
        app.update();
        let baseline = snapshot(app.world_mut());
        assert_eq!(baseline.len(), sources.len());
        // Move sources into different archetypes and change their materials.
        for (i, &source) in sources.iter().enumerate().rev().step_by(2) {
            app.world_mut().entity_mut(source).insert((
                crate::actors::npc::Npc { actor: i as u32, ..default() }, MapIcon("ui/icon_map_questionmark".into()),
            ));
        }
        for _ in 0..3 {
            app.update();
            assert_eq!(snapshot(app.world_mut()), baseline);
        }
        // Reopening/resizing the map creates new marker entities.
        let mut query = app.world_mut().query_filtered::<Entity, With<Marker>>();
        let markers: Vec<_> = query.iter(app.world()).collect();
        for marker in markers { app.world_mut().despawn(marker); }
        app.world_mut().get_mut::<MapGeometry>(map).unwrap().view = View::new(MapMode::Expanded, BASE_W);
        app.update();
        assert_eq!(snapshot(app.world_mut()), baseline);
        app.world_mut().despawn(sources[32]);
        app.update();
        let order: Vec<_> = snapshot(app.world_mut()).into_iter().map(|(target, _)| target).collect();
        assert_eq!(order, baseline.into_iter().map(|(target, _)| target).filter(|&target| target != sources[32]).collect::<Vec<_>>());
    }

    #[test]
    fn monster_markers_disappear_at_death_before_the_corpse_is_removed() {
        use crate::actors::{character::CharacterAnim, monsters::{Monster, tests::monster}};
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Image>().init_asset::<Mesh>().init_asset::<UiMaterial>().init_asset::<super::super::MapNames>()
            .add_systems(Update, update);
        let map = app.world_mut().spawn(MapGeometry {
            source: Handle::<NavMesh>::default().id(), view: View::new(MapMode::Compact, BASE_W),
            projection: GroundProjection::gameplay(), min: Vec2::ZERO, origin: Vec2::ZERO, scale: 2.0,
        }).id();
        app.world_mut().spawn((MarkerLayer, ChildOf(map)));
        let monsters: Vec<_> = (41..44).map(|actor| app.world_mut().spawn((
            monster(actor, 100), CharacterAnim::default(), GlobalTransform::IDENTITY,
            crate::network::net::RemotePlayer { actor, ..default() },
        )).id()).collect();
        app.update();
        let mut markers = app.world_mut().query::<&Marker>();
        assert_eq!(markers.iter(app.world()).count(), 3);
        app.world_mut().get_mut::<Monster>(monsters[0]).unwrap().health = 0;
        app.world_mut().get_mut::<CharacterAnim>(monsters[1]).unwrap().dead = true;
        app.update();
        assert_eq!(markers.single(app.world()).unwrap().target, monsters[2]);
        assert!(app.world().get_entity(monsters[0]).is_ok(), "corpse still exists");
        assert!(app.world().get_entity(monsters[1]).is_ok(), "death animation still exists");
        app.world_mut().despawn(monsters[2]);
        app.update();
        assert_eq!(markers.iter(app.world()).count(), 0);
    }
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub(crate) struct Style { pub(crate) icon: String, pub(crate) color: [u8; 3], size: u8 }

impl Style {
    fn authored(icon: &str) -> Option<Self> {
        (!icon.is_empty()).then(|| Self { icon: icon.replace('\\', "/"), color: [255; 3], size: 28 })
    }
    fn player() -> Self { Self { icon: "ui/icon_map_player".into(), color: [255; 3], size: 20 } }
    fn dot(hostile: bool) -> Self {
        Self { icon: "ui/icon_map_team".into(), color: if hostile { [255, 40, 40] } else { [35, 125, 255] }, size: 18 }
    }
}

#[derive(Component)]
pub(in crate::ui) struct MarkerLayer;

#[derive(Component)]
pub(in crate::ui) struct Marker { target: Entity, style: Style }

#[derive(Default)]
pub(in crate::ui) struct ArtCache(HashMap<Style, (Handle<Mesh>, Handle<UiMaterial>)>);

fn order_markers(wanted: &mut HashMap<Entity, (Style, Transform)>) {
    // Transparent2d sorts by global Z. Equal-depth icons can change order when
    // batches/materials change, even if the markers themselves have not moved.
    // Source entities keep their relative order across query/archetype changes
    // and marker rebuilds. Reserve [2, 3) for them, below the player at Z = 3.
    let mut targets: Vec<_> = wanted.iter()
        .filter(|(_, (_, at))| at.translation.z < 3.0)
        .map(|(&target, _)| target).collect();
    targets.sort_unstable();
    let step = 1.0 / (targets.len() + 1) as f32;
    for (rank, target) in targets.into_iter().enumerate() {
        wanted.get_mut(&target).unwrap().1.translation.z = 2.0 + rank as f32 * step;
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(in crate::ui) fn update(
    mut commands: Commands,
    layers: Query<(Entity, &ChildOf), With<MarkerLayer>>,
    geometry: Query<&MapGeometry>,
    sources: Query<(Entity, &GlobalTransform, Option<&MapIcon>, Option<&crate::actors::npc::Npc>,
        Option<&crate::network::net::RemotePlayer>, Option<&crate::actors::monsters::Monster>, Has<crate::network::net::LocalPlayer>, Option<&crate::actors::character::CharacterAnim>),
        (Without<Marker>, Or<(With<MapIcon>, With<crate::network::net::RemotePlayer>, With<crate::network::net::LocalPlayer>)>)>,
    mut markers: Query<(Entity, &mut Marker, &mut Transform, &mut Mesh2d, &mut MeshMaterial2d<UiMaterial>)>,
    labels: Query<&MapLabel>,
    (loaded, names): (Option<Res<crate::ui::Loaded>>, Res<Assets<super::MapNames>>),
    net: Option<NonSend<crate::network::net::Net>>,
    assets: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<UiMaterial>>,
    mut cache: Local<ArtCache>,
    screen: Option<Res<crate::ui::Screen>>,
    mut captions: Query<(&mut TextFont, &mut Transform), (With<ExitCaption>, Without<Marker>)>,
) {
    let scale = screen.as_ref().map_or(1.0, |s| s.scale);
    // Rasterize at the actual screen resolution. The map art uses reference
    // units, which otherwise magnify a small glyph atlas on large windows.
    let Ok((parent, child)) = layers.single() else { return };
    let Ok(map) = geometry.get(child.parent()) else { return };
    // The icons shrink with the compact map (230 of the expanded 520
    // reference pixels high), as its art does; their captions keep their size.
    // FAILURE: "quand la map est en petit les icones sont trop gros".
    let shrink = (map.view.size.y / 520.0).clamp(0.55, 1.0);
    for (mut font, mut transform) in &mut captions {
        let size = bevy::text::FontSize::Px(9.0 * scale);
        if font.font_size != size { font.font_size = size; }
        let inverse = Vec3::new(1.0, -1.0, 1.0) / (scale * shrink);
        if transform.scale != inverse { transform.scale = inverse; }
    }
    let empty = MapMarkers::default();
    let state = net.as_ref().map(|n| &n.map_markers).unwrap_or(&empty);
    let mut wanted = HashMap::new();
    for (entity, at, icon, npc, remote, monster, local, anim) in &sources {
        if anim.is_some_and(|a| a.dead) || monster.is_some_and(|m| m.health == 0) { continue; }
        let style = if local { Some(Style::player()) }
        else if let Some(npc) = npc { icon.and_then(|i| state.npc_style(npc.actor, &i.0)) }
        // A summon of ours or a peer's outside PvP is a friend's dot.
        else if let Some(remote) = remote { Some(Style::dot(state.hostile(remote.actor, monster.is_some_and(|m| m.targetable)))) }
        else { icon.and_then(|i| Style::authored(&i.0)) };
        if let Some(style) = style {
            let position = map.project(at.translation());
            if position.is_finite() { wanted.insert(entity, (style, Transform::from_translation(position.extend(if local { 3.0 } else { 2.0 })).with_scale(Vec3::splat(shrink)))); }
        }
    }
    order_markers(&mut wanted);
    let mut art = |style: &Style| {
        cache.0.entry(style.clone()).or_insert_with(|| {
            let half = style.size as f32 * 0.5;
            let mut run = Run::default();
            polygon(&mut run, &[Vec2::new(-half, -half), Vec2::new(half, -half), Vec2::new(half, half), Vec2::new(-half, half)],
                [style.color[0] as f32 / 255.0, style.color[1] as f32 / 255.0, style.color[2] as f32 / 255.0, 1.0]);
            run.uv = vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
            (meshes.add(run.mesh()), materials.add(material(Some(assets.load(format!("textures/{}.png", style.icon))))))
        }).clone()
    };
    for (entity, mut marker, mut transform, mut mesh, mut mat) in &mut markers {
        let Some((style, at)) = wanted.remove(&marker.target) else { commands.entity(entity).despawn(); continue };
        *transform = at;
        if marker.style != style {
            let (m, material) = art(&style);
            mesh.0 = m;
            mat.0 = material;
            marker.style = style;
        }
    }
    let names = loaded.as_ref().and_then(|l| names.get(&l.map_names));
    for (target, (style, transform)) in wanted {
        let half = style.size as f32 * 0.5;
        let (mesh, mat) = art(&style);
        let marker = commands.spawn((Marker { target, style }, Mesh2d(mesh), MeshMaterial2d(mat), transform,
            Visibility::Inherited, RenderLayers::layer(LAYER), ChildOf(parent))).id();
        if let (Ok(label), Some(names)) = (labels.get(target), names) {
            commands.spawn((
                ExitCaption,
                Text2d::new(names.caption(&label.0)),
                TextFont { font: assets.load("fonts/tahoma.ttf").into(), font_size: bevy::text::FontSize::Px(9.0 * scale), ..default() },
                TextColor(Color::WHITE),
                // The marker layer is y-flipped (its art's y runs down): the text
                // is flipped back, and +y is below the symbol.
                Transform::from_xyz(0.0, half + 6.0, 0.1).with_scale(Vec3::new(1.0, -1.0, 1.0) / scale),
                RenderLayers::layer(LAYER),
                ChildOf(marker),
            ));
        }
    }
}

#[derive(Component)]
pub(in crate::ui) struct ExitCaption;
