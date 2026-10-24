//! Database-ordered spell cards and pointer-owned hotbar edits. The server owns
//! availability; saving a slot always sends the complete release261 bar.
use super::*;
use dsor_proto::commands::{inventory::{QuickSlot as Slot, QuickSlots, SkillBookEntry}, ClientCommand};

const BAR_SIZE: usize = 17;
const VISIBLE_SLOTS: usize = 8;

#[derive(Clone, Deserialize, Debug)]
pub(in crate::ui) struct Spell {
    pub id: String,
    pub wire: Option<u32>,
    pub class_id: u8,
    pub unlock: u32,
    pub name: String,
    pub icon: String,
    #[serde(default)] pub description: String,
    #[serde(default)] pub target: String,
    /// Authored locale tokens resolved against unmodified skill database values.
    #[serde(default)] pub preview_description: String,
    #[serde(default)] pub growth: String,
    #[serde(default)] pub talent_cost: u32,
}

#[derive(Clone, PartialEq)]
pub(in crate::ui) struct Model {
    pub class: u8,
    pub offline: bool,
    pub ready: bool,
    pub active: usize,
    pub book: HashMap<u32, SkillBookEntry>,
    pub bar: Vec<Option<String>>,
}

impl Default for Model {
    fn default() -> Self {
        Self { class: 1, offline: true, ready: true, active: 0, book: default(), bar: crate::network::net::default_bar(1) }
    }
}

impl Model {
    pub fn available(&self, spell: &Spell) -> bool {
        spell.class_id == self.class && spell.wire.is_some_and(|wire| self.offline ||
            self.book.get(&wire).is_some_and(|entry| entry.owned && !entry.unknown_flag_1))
    }
}

/// The spell model, handed to the windows when it changes and to those (re)built
/// since: not cloned into every window every frame (docs/ui.md, "Per-frame cost").
pub(in crate::ui) fn sync(
    net: Option<NonSend<crate::network::net::Net>>,
    players: Query<&crate::actors::character::Character, With<crate::network::net::LocalPlayer>>,
    mut windows: Query<&mut WindowControls>,
    hotbar: Res<hotbar::Hotbar>,
    mut last: Local<Option<Model>>,
) {
    let class = players.iter().next().map(|c| c.desc.class)
        .or_else(|| net.as_ref().and_then(|n| n.offline_pending.as_ref().map(|c| c.class))).unwrap_or(1);
    let model = if let Some(net) = net {
        Model { class, offline: net.offline, active: hotbar.active,
            ready: net.offline || net.quickbars.bars.get(hotbar.active).is_some_and(|b| b.len() == BAR_SIZE),
            book: net.skill_book.as_ref().map(|b| b.entries.iter().map(|e| (e.skill, *e)).collect()).unwrap_or_default(),
            bar: net.bar.clone() }
    } else { Model { class, ..default() } };
    let changed = last.as_ref() != Some(&model);
    // is_changed: added or rebuilt by another system (this one's writes don't count).
    for mut c in &mut windows {
        if (changed || c.is_changed()) && c.spells != model { c.spells = model.clone(); }
    }
    if changed { *last = Some(model); }
}

pub(super) struct DragSpell {
    root: Entity,
    button: usize,
    origin: Vec2,
    pub active: bool,
    spell: Spell,
    source: Option<usize>,
    bar: usize,
}

impl DragSpell {
    /// The bar and slot a spell is being carried from, once the drag is under
    /// way: that slot shows empty until it is let go (crate::ui update_slots).
    pub(super) fn lifted(&self) -> Option<(usize, usize)> {
        if !self.active { return None; }
        Some((self.bar, self.source?))
    }
}

#[cfg_attr(test, derive(Clone))]
pub(super) struct DropSpell {
    spell: Spell,
    source: Option<usize>,
    bar: usize,
    /// The slot it lands on; None: dragged off the bar, its source slot clears.
    target: Option<usize>,
}

/// Returns true when release belongs to a drag, so it cannot also cast a spell.
pub(super) fn drag(pointer: &mut Pointer, windows: &Windows, mouse: &ButtonInput<MouseButton>,
    hit: Option<(Entity, usize)>, over: Option<Entity>, point: Option<Vec2>, allowed: bool) -> bool {
    let bar = windows.iter().find(|(_, n, _, v, _)| n.0 == "bottombar" && **v != Visibility::Hidden);
    let editable = allowed && bar.is_some_and(|(_, _, c, _, _)| !c.locked && c.spells.ready);
    if mouse.just_pressed(MouseButton::Left) {
        pointer.spell_drag = None;
        if editable {
            if let (Some((root, button)), Some(point)) = (hit, point) {
                let (_, _, c, _, _) = windows.get(root).unwrap();
                let b = &c.buttons[button];
                let (spell, source) = match b.action {
                    Action::Spell => (b.spell.clone().filter(|s| c.spells.available(s)), None),
                    Action::Skill(slot) => {
                        let id = c.spells.bar.get(slot).and_then(|id| id.as_ref());
                        let meta = windows.iter().flat_map(|(_, _, c, _, _)| c.buttons.iter())
                            .filter_map(|b| b.spell.as_ref()).find(|s| Some(&s.id) == id && s.class_id == c.spells.class);
                        (meta.cloned(), Some(slot))
                    }
                    _ => (None, None),
                };
                if let Some(spell) = spell {
                    pointer.spell_drag = Some(DragSpell { root, button, origin: point, active: false, spell, source, bar: c.spells.active });
                }
            }
        }
    }
    if pointer.spell_drag.as_ref().is_some_and(|d| !editable || !windows.get(d.root).is_ok_and(|(_, _, c, v, _)|
        *v != Visibility::Hidden && c.spells.class == d.spell.class_id && c.section_visible(c.buttons[d.button].section) && c.enabled(&c.buttons[d.button]))) {
        pointer.spell_drag = None;
        pointer.down = None;
    }
    if let Some(d) = &mut pointer.spell_drag {
        if point.is_some_and(|p| p.distance(d.origin) >= 4.0) { d.active = true; }
    }
    if mouse.just_released(MouseButton::Left) {
        if let Some(d) = pointer.spell_drag.take() {
            if d.active {
                if let Some((root, button)) = hit {
                    let (_, name, c, _, _) = windows.get(root).unwrap();
                    if name.0 == "bottombar" && editable {
                        if let Action::Skill(target) = c.buttons[button].action {
                            pointer.spell_drop = Some(DropSpell { spell: d.spell.clone(), source: d.source, target: Some(target), bar: d.bar });
                        }
                    }
                }
                if d.source.is_some() && point.is_some() && bar.is_some_and(|(root, _, _, _, _)| Some(root) != over) {
                    pointer.spell_drop = Some(DropSpell { spell: d.spell, source: d.source, target: None, bar: d.bar });
                }
                pointer.down = None;
                return true;
            }
        }
    }
    if !mouse.pressed(MouseButton::Left) { pointer.spell_drag = None; }
    false
}

/// Refuse incomplete snapshots and stale source slots rather than replacing
/// unseen item/essence slots with guessed empty entries.
fn edited_bar(slots: &[Slot], drop: &DropSpell) -> Option<Vec<Slot>> {
    if slots.len() != BAR_SIZE || drop.target.is_some_and(|t| t >= VISIBLE_SLOTS) { return None; }
    let mut edited = slots.to_vec();
    match (drop.source, drop.target) {
        (Some(source), target) => {
            if source >= VISIBLE_SLOTS || slots[source].kind != 0 || slots[source].id.as_ref() != Some(&drop.spell.id) { return None; }
            match target {
                Some(target) => edited.swap(source, target),
                None => edited[source] = Slot { kind: Slot::EMPTY, id: None },
            }
        }
        (None, Some(target)) => edited[target] = Slot { kind: 0, id: Some(drop.spell.id.clone()) },
        (None, None) => return None,
    }
    (edited != slots).then_some(edited)
}

pub(in crate::ui) fn apply_drop(mut pointer: ResMut<Pointer>, net: Option<NonSendMut<crate::network::net::Net>>) {
    let Some(mut net) = net else { return };
    // A talent card buys one rank; a preset button switches the category's preset
    // (DSOR_Server services/talents.py answers with the effects and the selection,
    // or a snapshot when it refuses).
    if let Some(talent) = pointer.talent.take() {
        if !net.offline { net.send(&dsor_proto::commands::ClientCommand::SelectTalent(dsor_proto::commands::inventory::SelectTalent { talent, ranks: 1 })); }
    }
    if let Some((preselection, category)) = pointer.preset.take() {
        if !net.offline { net.send(&dsor_proto::commands::ClientCommand::TalentPreselection(dsor_proto::commands::inventory::TalentPreselection { preselection, category })); }
    }
    let Some(drop) = pointer.spell_drop.take() else { return };
    if drop.source.is_none() && !net.offline && !drop.spell.wire.is_some_and(|wire|
        net.skill_book.as_ref().is_some_and(|b| b.entries.iter().any(|e| e.skill == wire && e.owned && !e.unknown_flag_1))) { return; }
    if net.offline && net.quickbars.bars.is_empty() {
        let bar = (0..BAR_SIZE).map(|i| {
            let id = net.bar.get(i).cloned().flatten();
            Slot { kind: if id.is_some() { 0 } else { Slot::EMPTY }, id }
        }).collect();
        net.quickbars.bars.push(bar);
    }
    let Some(slots) = net.quickbars.bars.get(drop.bar).and_then(|bar| edited_bar(bar, &drop)) else { return };
    net.bar = slots.iter().map(|s| if s.kind == 0 { s.id.clone() } else { None }).collect();
    net.quickbars.bars[drop.bar] = slots.clone();
    net.send(&ClientCommand::QuickSlots(QuickSlots { slots, bar: drop.bar as u32 }));
}

#[derive(Component)]
pub(in crate::ui) struct SpellOverlay;

/// This layer has no input region. It follows the cursor in reference pixels,
/// including while a window is moved or the viewport changes size.
pub(in crate::ui) fn overlay(
    mut commands: Commands, pointer: Res<Pointer>,
    primary: Query<&Window, With<PrimaryWindow>>, screen: Res<Screen>, assets: Res<AssetServer>,
    old: Query<Entity, With<SpellOverlay>>, mut last: Local<Option<(String, bool, u32)>>, views: Res<super::systems::Views>,
    net: Option<NonSend<crate::network::net::Net>>, (data, catalogs): (Res<super::items::Data>, Res<Assets<super::items::Catalog>>),
    mut transforms: Query<&mut Transform, With<SpellOverlay>>,
) {
    let window = primary.single().ok();
    let point = window.filter(|w| w.focused && w.height() > 0.0)
        .and_then(|w| w.cursor_position().map(|p| p * (BASE_H / w.height())));
    let dragged = pointer.spell_drag.as_ref().filter(|d| d.active).map(|d| &d.spell);
    let description = dragged.map(|s| (s, true));
    // A Collector's Bag sticker dragged to the bar (super::stickers): its icon.
    let sticker = super::stickers::dragged(&pointer, &views, net.as_deref(), catalogs.get(&data.catalog)).filter(|_| dragged.is_none());
    let sticker = sticker.as_deref();
    let key = description.map(|(s, available)| (format!("{}:{}", s.id, available), dragged.is_some(), screen.scale.to_bits()))
        .or_else(|| sticker.map(|icon| (format!("sticker:{icon}"), true, screen.scale.to_bits())));
    let position = point.map(|p| if dragged.is_some() || sticker.is_some() { p } else { Vec2::new((p.x + 20.0).min(screen.width - 265.0).max(0.0), (p.y + 20.0).min(BASE_H - 75.0)) });
    if *last != key || point.is_none() {
        for e in &old { commands.entity(e).despawn(); }
        *last = if point.is_some() { key } else { None };
        if let (Some(icon), Some(p)) = (sticker, position) {
            commands.spawn((SpellOverlay, Sprite { image: assets.load(icon.to_owned()), custom_size: Some(Vec2::splat(34.0)), ..default() },
                Transform::from_translation(screen.world(0.0, p.x, p.y).extend(800.0)), RenderLayers::layer(LAYER)));
        }
        if let (Some((s, available)), Some(p)) = (description, position) {
            let tf = Transform::from_translation(screen.world(0.0, p.x, p.y).extend(800.0));
            if dragged.is_some() {
                commands.spawn((SpellOverlay, Sprite { image: assets.load(s.icon.clone()), custom_size: Some(Vec2::splat(34.0)), ..default() },
                    tf, RenderLayers::layer(LAYER)));
            } else {
                let hint = if s.wire.is_none() { "Passive talent".to_owned() } else if available { "Drag to the action bar".to_owned() }
                    else { format!("Level {} — unavailable", s.unlock) };
                let root = commands.spawn((SpellOverlay, tf, Visibility::Inherited)).id();
                commands.spawn((Sprite::from_color(Color::srgba(0.035, 0.025, 0.02, 0.96), Vec2::new(260.0, 66.0)),
                    Anchor::TOP_LEFT, RenderLayers::layer(LAYER), ChildOf(root)));
                commands.spawn((Text2d::new(format!("{}\n{}", s.name, hint)), TextFont { font: assets.load("fonts/tahoma.ttf").into(),
                    font_size: bevy::text::FontSize::Px(13.0 * TEXT_SCALE * screen.scale), ..default() }, TextColor(Color::srgb(1.0, 0.88, 0.62)),
                    bevy::text::TextBounds::new_horizontal(244.0 * screen.scale), Anchor::TOP_LEFT,
                    Transform::from_xyz(8.0, -7.0, 1.0).with_scale(Vec3::splat(1.0 / screen.scale)), RenderLayers::layer(LAYER), ChildOf(root)));
            }
        }
    } else if let Some(p) = position {
        for mut tf in &mut transforms { tf.translation = screen.world(0.0, p.x, p.y).extend(800.0); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn spell() -> Spell { Spell { id: "magicmissile".into(), wire: Some(42), class_id: 1, unlock: 1, name: "Magic missile".into(), icon: String::new(), description: String::new(), target: String::new(), preview_description: String::new(), growth: String::new(), talent_cost: 0 } }

    #[test]
    fn availability_uses_the_server_book_and_class_not_just_level() {
        let mut model = Model { offline: false, ..default() };
        assert!(!model.available(&spell()));
        model.book.insert(42, SkillBookEntry { skill: 42, owned: true, unknown_flag_1: false, unknown_flag_2: false });
        assert!(model.available(&spell()));
        model.book.get_mut(&42).unwrap().unknown_flag_1 = true;
        assert!(!model.available(&spell()));
        model.offline = true;
        model.class = 2;
        assert!(!model.available(&spell()));
    }

    #[test]
    fn a_spell_carried_from_the_bar_lifts_its_slot_once_the_drag_starts() {
        let drag = |active, source| DragSpell { root: Entity::PLACEHOLDER, button: 0, origin: Vec2::ZERO, active, spell: spell(), source, bar: 1 };
        assert_eq!(drag(false, Some(4)).lifted(), None);
        assert_eq!(drag(true, Some(4)).lifted(), Some((1, 4)));
        assert_eq!(drag(true, None).lifted(), None);
    }

    #[test]
    fn replacing_and_swapping_preserve_unseen_slots_and_reject_stale_drags() {
        let mut slots = vec![Slot { kind: -1, id: None }; BAR_SIZE];
        slots[0] = Slot { kind: 0, id: Some("magicmissile".into()) };
        slots[2] = Slot { kind: 1, id: Some("healing_potion".into()) };
        slots[16] = Slot { kind: 2, id: Some("essence".into()) };
        let drop = DropSpell { bar: 0, spell: spell(), source: None, target: Some(3) };
        let edited = edited_bar(&slots, &drop).unwrap();
        assert_eq!(edited[3], slots[0]);
        for i in 0..BAR_SIZE { if i != 3 { assert_eq!(edited[i], slots[i]); } }
        let swap = DropSpell { source: Some(0), target: Some(2), ..drop.clone() };
        let edited = edited_bar(&slots, &swap).unwrap();
        assert_eq!(edited[0], slots[2]);
        assert_eq!(edited[2], slots[0]);
        assert_eq!(edited[16], slots[16]);
        assert!(edited_bar(&edited, &swap).is_none());
        assert!(edited_bar(&slots[..7], &swap).is_none());
        // Dragged off the bar: only its own slot clears; a stale source or a
        // book spell with nowhere to go changes nothing.
        let off = DropSpell { source: Some(0), target: None, ..drop.clone() };
        let edited = edited_bar(&slots, &off).unwrap();
        assert_eq!(edited[0], Slot { kind: Slot::EMPTY, id: None });
        for i in 1..BAR_SIZE { assert_eq!(edited[i], slots[i]); }
        assert!(edited_bar(&edited, &off).is_none());
        assert!(edited_bar(&slots, &DropSpell { source: Some(2), target: None, ..drop.clone() }).is_none());
        assert!(edited_bar(&slots, &DropSpell { source: None, target: None, ..drop }).is_none());
    }

    #[test]
    fn applying_a_drop_updates_the_display_and_preserves_other_bars() {
        let mut app = App::new();
        app.init_resource::<Time<Real>>().init_resource::<Pointer>().add_systems(Update, apply_drop);
        crate::network::net::start_offline(app.world_mut(), 1, 0);
        let mut slots = vec![Slot { kind: Slot::EMPTY, id: None }; BAR_SIZE];
        slots[16] = Slot { kind: 2, id: Some("essence".into()) };
        let other = vec![Slot { kind: 1, id: Some("item".into()) }; BAR_SIZE];
        app.world_mut().non_send_mut::<crate::network::net::Net>().quickbars.bars = vec![slots.clone(), other.clone()];
        app.world_mut().resource_mut::<Pointer>().spell_drop = Some(DropSpell { bar: 0, spell: spell(), source: None, target: Some(3) });
        app.update();
        let net = app.world().non_send::<crate::network::net::Net>();
        assert_eq!(net.bar[3].as_deref(), Some("magicmissile"));
        assert_eq!(net.quickbars.bars[0].len(), BAR_SIZE);
        assert_eq!(net.quickbars.bars[0][16], slots[16]);
        assert_eq!(net.quickbars.bars[1], other);
        assert!(app.world().resource::<Pointer>().spell_drop.is_none());
    }

    #[test]
    fn removing_a_spell_clears_only_its_slot_and_rejects_stale_sources() {
        let mut slots = vec![Slot { kind: 1, id: Some("potion".into()) }; BAR_SIZE];
        slots[0] = Slot { kind: 0, id: Some(spell().id) };
        let drop = DropSpell { bar: 0, spell: spell(), source: Some(0), target: None };
        let edited = edited_bar(&slots, &drop).unwrap();
        assert_eq!(edited[0], Slot { kind: Slot::EMPTY, id: None });
        assert_eq!(&edited[1..], &slots[1..]);
        assert!(edited_bar(&edited, &drop).is_none());
        assert!(edited_bar(&slots[..8], &drop).is_none());
        assert!(edited_bar(&slots, &DropSpell { source: None, ..drop }).is_none());
    }

    #[test]
    fn dragging_out_clears_a_slot_but_bar_background_and_cancelled_drags_do_not() {
        use bevy::ecs::system::RunSystemOnce;
        for outcome in ["outside", "bar_background", "locked", "unfocused", "missing_cursor"] {
            let mut world = World::new();
            world.init_resource::<Time<Real>>();
            world.init_resource::<Pointer>();
            crate::network::net::start_offline(&mut world, 1, 0);
            let layout: Layout = serde_json::from_value(serde_json::json!({"window":"bottombar","widgets":[
                {"id":"Canvas/LMB","type":"SlotButton","k":0.0,"rect":[0,0,30,30],"hidden":false}
            ]})).unwrap();
            let mut controls = WindowControls::new(&layout);
            // The real spell book supplies this metadata, even while hidden.
            controls.buttons[0].spell = Some(spell());
            controls.locked = outcome == "locked";
            let bar = world.spawn((Window2018("bottombar".into()),controls,Visibility::Inherited,Transform::default())).id();
            world.run_system_once(move |mut pointer: ResMut<Pointer>, windows: Windows| {
                let mut mouse = ButtonInput::default();
                mouse.press(MouseButton::Left);
                assert!(!drag(&mut pointer,&windows,&mouse,Some((bar,0)),Some(bar),Some(Vec2::ZERO),true));
            }).unwrap();
            let consumed = world.run_system_once(move |mut pointer: ResMut<Pointer>, windows: Windows| {
                let mut mouse = ButtonInput::default();
                mouse.press(MouseButton::Left); mouse.clear(); mouse.release(MouseButton::Left);
                drag(&mut pointer,&windows,&mouse,None,(outcome=="bar_background").then_some(bar),
                    (outcome!="missing_cursor").then_some(Vec2::splat(100.)),outcome!="unfocused")
            }).unwrap();
            assert_eq!(consumed, matches!(outcome,"outside"|"bar_background"));
            world.run_system_once(apply_drop).unwrap();
            let net = world.non_send::<crate::network::net::Net>();
            assert_eq!(net.bar[0].as_deref(), if outcome=="outside" { None } else { Some("magicmissile") });
            if outcome=="outside" {
                assert_eq!(net.quickbars.bars[0][0],Slot { kind:Slot::EMPTY,id:None });
            }
        }
    }
}
