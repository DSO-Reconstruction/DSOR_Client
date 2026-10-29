//! Opt-in native pointer/capture fixture for focus and overlap regressions.
use super::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};

#[derive(Deserialize)]
struct Step {
    frame: u32,
    /// Logical window dimensions, for responsive native UI captures.
    window_size: Option<[u32; 2]>,
    /// Reference pixels on a 1024x768 screen, centered at wider aspect ratios.
    point: Option<[f32; 2]>,
    /// Reference pixels anchored to the screen's left edge.
    left_point: Option<[f32; 2]>,
    /// Click the live NPC's torso through the normal picking/movement path.
    npc: Option<String>,
    /// Offline-only fixture placement and quest offer; never sent to a server.
    player: Option<[f32; 3]>,
    npc_quest: Option<String>,
    npc_quest_state: Option<u8>,
    /// With a state: the quest log's finished tasks (bit n: task n), for the
    /// quest screen's check marks.
    npc_quest_done: Option<u16>,
    npc_offers: Option<Vec<String>>,
    /// With npc_offers: their Andermant prices, offer by offer (0 past the list).
    npc_offers_rc: Option<Vec<u32>>,
    /// Offline-only wallet, CurrencyChanged's balances (Andermant, gold in
    /// copper, Honor, legacy, Draken): the trader's greyed offers.
    wallet: Option<[u64; 5]>,
    /// Replay a server Offer packet through the real decoder, offline only.
    npc_offer_packet: Option<String>,
    /// Offline tooltip fixture, decoded through the ordinary inventory protocol.
    inventory_packet: Option<String>,
    /// Hold/release Y across captures to inspect the advanced item card.
    advanced_tooltip: Option<bool>,
    key: Option<String>,
    left: Option<bool>,
    /// Press (true) or release the right button (a desk's pick of an item).
    right: Option<bool>,
    capture: Option<String>,
    #[serde(default)] quit: bool,
}

#[derive(Default)]
pub(super) struct Replay {
    loaded: bool,
    frame: u32,
    steps: std::collections::VecDeque<Step>,
    quit_at: Option<u32>,
    key: Option<KeyCode>,
}

pub(super) fn run(mut commands: Commands, mut replay: Local<Replay>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>, mut mouse: ResMut<ButtonInput<MouseButton>>,
    roots: Query<(Entity, &Window2018, &Transform, &Visibility), Without<crate::network::net::LocalPlayer>>, children: Query<&Children>,
    elements: Query<(Option<&GlobalTransform>, Option<&Visibility>, Option<&InheritedVisibility>, Option<&ViewVisibility>, Option<&Text2d>, Has<Mesh2d>, Has<Sprite>)>,
    npcs: Query<(&crate::actors::npc::Npc, &GlobalTransform)>,
    cameras: Query<(&Camera, &GlobalTransform), (With<Camera3d>, Without<crate::rendering::highlight::HighlightCamera>)>,
    mut players: Query<(&mut crate::network::net::LocalPlayer, &mut Transform)>,
    mut net: Option<NonSendMut<crate::network::net::Net>>, mut keys: ResMut<ButtonInput<KeyCode>>) {
    if !replay.loaded {
        replay.loaded = true;
        if let Ok(path) = std::env::var("DSOR_UI_REPLAY") {
            match std::fs::read(path).map_err(|e| e.to_string()).and_then(|b| serde_json::from_slice(&b).map_err(|e| e.to_string())) {
                Ok(steps) => replay.steps = steps,
                Err(e) => error!("UI replay: {e}"),
            }
        }
    }
    if replay.steps.is_empty() && replay.quit_at.is_none() { return; }
    if let Some(key) = replay.key.take() { keys.release(key); }
    let Ok(mut window) = windows.single_mut() else { return };
    replay.frame += 1;
    if replay.quit_at.is_some_and(|at| replay.frame >= at) { commands.write_message(AppExit::Success); return; }
    while replay.steps.front().is_some_and(|s| s.frame <= replay.frame) {
        let step = replay.steps.pop_front().unwrap();
        if let Some([width, height]) = step.window_size {
            window.resolution.set(width as f32, height as f32);
        }
        if let Some(name) = step.key.as_deref() {
            if let Some(key) = match name { "Escape" => Some(KeyCode::Escape), "F1" => Some(KeyCode::F1), "V" => Some(KeyCode::KeyV), "I" => Some(KeyCode::KeyI), "C" => Some(KeyCode::KeyC), "Q" => Some(KeyCode::KeyQ), "U" => Some(KeyCode::KeyU), "E" => Some(KeyCode::KeyE), "L" => Some(KeyCode::KeyL), "J" => Some(KeyCode::KeyJ), "F" => Some(KeyCode::KeyF), "9" => Some(KeyCode::Digit9), "S" => Some(KeyCode::KeyS), "N" => Some(KeyCode::KeyN), "F8" => Some(KeyCode::F8), "Down" => Some(KeyCode::ArrowDown), "Tab" => Some(KeyCode::Tab), "Delete" => Some(KeyCode::Delete), _ => None } {
                keys.press(key);
                replay.key = Some(key);
            }
        }
        if net.as_ref().is_some_and(|n| n.offline) {
            if let Some(held) = step.advanced_tooltip {
                if held { keys.press(KeyCode::KeyY); } else { keys.release(KeyCode::KeyY); }
            }
            if let (Some(path),Some(net))=(&step.inventory_packet,net.as_mut()) {
                match std::fs::read(path).map_err(|e|e.to_string()).and_then(|bytes|
                    dsor_proto::commands::decode_message(&bytes,bytes.len()*8).map_err(|e|e.to_string())) {
                    Ok(commands)=>for (command,_) in commands {
                        if let dsor_proto::commands::ServerCommand::InventoryInfo(info)=command {net.inventory.apply(info);}
                    },
                    Err(e)=>error!("Inventory tooltip replay: {e}"),
                }
            }
            if let Some(position) = step.player {
                for (mut player, mut at) in &mut players { at.translation = Vec3::from(position); player.target = None; }
            }
            if let (Some(wallet), Some(net)) = (step.wallet, net.as_mut()) { net.wallet = Some(wallet); net.touch_ui(); }
        }
        if let Some(template) = &step.npc {
            if let Some((npc, at)) = npcs.iter().find(|(n, _)| &n.template == template) {
                if let (Some(quest), Some(net)) = (&step.npc_quest, net.as_mut().filter(|n| n.offline)) {
                    let info = dsor_proto::commands::actors::NpcInfo {
                        name: npc.profile.shop.clone(), enabled: true,
                        interactions: vec![dsor_proto::commands::actors::NpcInteraction { name: quest.clone(), quest: quest.clone(), kind: 0, state: 10 }], ..default()
                    };
                    let command = dsor_proto::commands::ServerCommand::NpcInfo(info.clone());
                    net.map_markers.receive(&command, Some(npc.actor));
                    net.npc_dialogue.infos.insert(npc.actor, info);
                    if let Some(state) = step.npc_quest_state {
                        let command = dsor_proto::commands::ServerCommand::QuestUpdate(dsor_proto::commands::actors::QuestUpdate {
                            kind: 2, ints: vec![(2, state as u32)], strings: vec![(0, quest.clone())],
                        });
                        net.map_markers.receive(&command, None);
                        net.npc_dialogue.quests.insert(quest.to_lowercase(), state);
                        let entry = net.npc_dialogue.log.entry(quest.to_lowercase()).or_default();
                        entry.state = state;
                        entry.finished = step.npc_quest_done.unwrap_or(0);
                    }
                }
                if let (Some(items), Some(net)) = (&step.npc_offers, net.as_mut().filter(|n| n.offline)) {
                    use dsor_proto::commands::inventory::{Offer, ShopOffer};
                    let shop = npc.profile.shop.clone();
                    net.npc_dialogue.offers.insert(shop.clone(), Offer { shop,
                        offers: items.iter().enumerate().map(|(i, item)| Some(ShopOffer {
                            id: format!("preview-{i}"), item: item.clone(), amount: (i + 1) as u32,
                            price_vc: (i as u32 + 1) * 125, price_rc: step.npc_offers_rc.as_ref().and_then(|p| p.get(i)).copied().unwrap_or(0), ..default()
                        })).collect(), ..default()
                    });
                }
                if let (Some(path), Some(net)) = (&step.npc_offer_packet, net.as_mut().filter(|n| n.offline)) {
                    match std::fs::read(path).map_err(|e| e.to_string()).and_then(|bytes|
                        dsor_proto::commands::decode_message(&bytes, bytes.len() * 8).map_err(|e| e.to_string())) {
                        Ok(commands) => for (command, actor) in commands { net.npc_dialogue.receive(&command, actor); },
                        Err(e) => error!("NPC shop replay: {e}"),
                    }
                }
                // The fixtures above edit what the windows show, as a server would.
                if let Some(net) = net.as_mut().filter(|n| n.offline) { net.touch_ui(); }
                if let Ok((camera, transform)) = cameras.single() {
                    if let Ok(point) = camera.world_to_viewport(transform, at.translation() + Vec3::Y * 1.1) {
                        let window = window.bypass_change_detection();
                        window.set_cursor_position(Some(point));
                        window.focused = true;
                        info!("NPC replay: clicking {} actor {} at {:?}", template, npc.actor, point);
                    }
                }
            } else { error!("NPC replay: missing {template}"); }
        }
        if let Some([x, y]) = step.point {
            // Inject game input without asking winit to warp the desktop cursor.
            let window = window.bypass_change_detection();
            let scale = window.height() / BASE_H;
            let point = Vec2::new(x * scale + (window.width() - BASE_W * scale) * 0.5, y * scale);
            window.set_cursor_position(Some(point));
            window.focused = true;
        }
        if let Some([x, y]) = step.left_point {
            let window = window.bypass_change_detection();
            window.set_cursor_position(Some(Vec2::new(x, y) * (window.height() / BASE_H)));
            window.focused = true;
        }
        if let Some(right) = step.right {
            if right { mouse.press(MouseButton::Right); } else { mouse.release(MouseButton::Right); }
        }
        if let Some(left) = step.left {
            if left { mouse.press(MouseButton::Left); } else { mouse.release(MouseButton::Left); }
        }
        if let Some(path) = step.capture {
            for (player, at) in &players {
                info!("NPC replay frame {}: player {:?}, target {:?}", replay.frame, at.translation, player.target);
            }
            let mut snapshot = Vec::new();
            for (root, name, tf, visible) in &roots {
                if is_menu(&name.0) && *visible != Visibility::Hidden {
                    info!("UI replay frame {}: {} at {:?}", replay.frame, name.0, tf.translation);
                    let mut queue = vec![root];
                    while let Some(e) = queue.pop() {
                        if let Ok(c) = children.get(e) { queue.extend(c.iter()); }
                        if let Ok((transform, visibility, inherited, view, text, mesh, sprite)) = elements.get(e) {
                            if mesh || sprite || text.is_some() {
                                snapshot.push(serde_json::json!({"window":name.0,"entity":format!("{e:?}"),
                                    "position":transform.map(|t|t.translation().to_array()),"visibility":format!("{visibility:?}"),
                                    "inherited":inherited.map(|v|v.get()),"in_view":view.map(|v|v.get()),
                                    "text":text.map(|t|&t.0),"mesh":mesh,"sprite":sprite}));
                            }
                        }
                    }
                }
            }
            if let Err(e) = std::fs::write(std::path::Path::new(&path).with_extension("ui.json"), serde_json::to_vec_pretty(&snapshot).unwrap()) {
                error!("UI replay snapshot: {e}");
            }
            commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
        }
        if step.quit { replay.quit_at = Some(replay.frame + 30); }
    }
}
