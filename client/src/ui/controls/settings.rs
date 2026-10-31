//! Local preferences and the original settings window. Edits remain a draft
//! until Apply succeeds; shortcuts, captions and tooltips share one binding map.
use super::*;
use serde::Serialize;
use crate::rendering::graphics::{GraphicsSettings, DlssAvailability};

pub(crate) const WINDOW: &str = "settingswindow2";
pub(super) const ROWS: usize = 7;

#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub(crate) struct Preferences {
    pub tooltips: bool,
    pub advanced_tooltips: bool,
    pub own_name: bool,
    pub other_names: bool,
    pub combat_text: bool,
    pub camera_shake: bool,
    /// The hovered monster's negative status effects under its health bar
    /// (crate::ui::controls::effects).
    pub monster_effects: bool,
    pub attack_hovered: bool,
    pub classic_cursor: bool,
    pub lock_bar: bool,
    pub fullscreen: bool,
    pub vsync: bool,
    pub graphics: GraphicsSettings,
    pub map_transparency: u8,
    pub audio_muted: bool,
    pub master_volume: u8,
    pub music_volume: u8,
    pub ambient_volume: u8,
    pub effects_volume: u8,
    /// The game bar's comeback event buttons: Return Link (ExclsiveactiveCodeBtn)
    /// and Return Check-in (RecalledSignInBtn), shown or hidden.
    pub return_link: bool,
    pub return_checkin: bool,
    /// The currency the currency bar shows beside the money, picked in its bag
    /// (crate::ui::controls::currency): a currency id; empty, the honour.
    pub currency: String,
    /// The confirmation dialogs the player asked not to see again, by the
    /// original's DialogId (its settings' list, ClientSettingsManager +0x198):
    /// 6, a large Andermant purchase (crate::ui::controls::consent).
    pub skipped_dialogs: Vec<u32>,
    pub bindings: HashMap<String, String>,
    /// The revision of the default bindings the file was saved with (0: saved
    /// before revisions); `migrate` moves older files to the later defaults.
    #[serde(default)]
    pub keymap: u8,
}

/// 1 (2026-10-21): Quests on Q, the top-left consumable on U (was U and Q).
/// 2 (2026-10-21): Events on E (the bottom-left consumable moves to G), the
/// rankings on L, the battlegrounds on J.
/// 3 (2026-10-28): Tab swaps the right button's skill with the bar's TAB slot
/// (the one beside it); the map overlay moves to N.
/// FAILURE: "pour ouvrir la map c tab sauf que tab c'est pour switch le sort
///   du bas entre mon clic droit et celui de la case a cote".
const KEYMAP: u8 = 3;

/// The game bar's buttons a setting hides: each its own page group (shown on
/// page 0), named as the button (crate::ui::controls::section).
pub(super) const GAMEBAR_OPTIONAL: [&str; 2] = ["ExclsiveactiveCodeBtn", "RecalledSignInBtn"];

impl Default for Preferences {
    fn default() -> Self {
        Self { tooltips: true, advanced_tooltips: false, own_name: true, other_names: true, combat_text: true,
            camera_shake: true, monster_effects: true, attack_hovered: true, classic_cursor: false, lock_bar: false, fullscreen: false,
            vsync: true, graphics: GraphicsSettings::default(), map_transparency: 45,
            audio_muted: false, master_volume: 80, music_volume: 60, ambient_volume: 60, effects_volume: 80,
            return_link: true, return_checkin: true, currency: String::new(), skipped_dialogs: Vec::new(),
            bindings: shortcuts().into_iter().map(|(id, _, key)| (id, key.to_owned())).collect(), keymap: KEYMAP }
    }
}

/// Empty bindings are intentional; absent bindings inherit newly added defaults.
pub(super) fn shortcuts() -> Vec<(String, String, &'static str)> {
    let mut result = vec![];
    for (slot, label, key) in [
        (0, "Primary attack (keyboard)", ""), (1, "Secondary attack (keyboard)", ""),
        (2, "Action slot 1", "Digit1"), (3, "Action slot 2", "Digit2"),
        (4, "Action slot 3", "Digit3"), (5, "Action slot 4", "Digit4"),
        (6, "Action slot 5", "Digit5"), (7, "Alternate attack", "Digit6"),
        (8, "Mount", "KeyO"), (9, "Companion", "KeyH"),
        (13, "Consumable: left top", "KeyU"),
        (14, "Consumable: left bottom", "KeyG"), (15, "Consumable: right top", "KeyR"),
        (16, "Consumable: right bottom", "KeyT"),
    ] { result.push((format!("slot{slot}"), label.into(), key)); }
    for (id, label, key) in [
        ("inventory", "Inventory", "KeyI"), ("charactersheet", "Character", "KeyC"),
        ("skilltalentswindow", "Skills", "KeyS"), ("collectorsalbum", "Collector's album", "KeyW"),
        ("questlog", "Quests", "KeyQ"), ("locationmap", "Map overlay", "KeyN"),
        ("swap_attack", "Swap secondary and alternate attack", "Tab"),
        ("travelmap", "World map", "KeyM"), ("socialscreen", "Social", "KeyF"),
        ("eventwindow", "Events", "KeyE"), ("leaderboards", "Rankings", "KeyL"),
        ("matchmakingwindow", "Battlegrounds", "KeyJ"),
        ("chat", "Expand / collapse chat", "KeyV"), ("essence", "Switch essences", "KeyX"),
        ("previous_bar", "Previous action bar", "BracketLeft"), ("next_bar", "Next action bar", "BracketRight"),
    ] { result.push((id.into(), label.into(), key)); }
    result
}

pub(super) fn key_name(key: KeyCode) -> String { format!("{key:?}") }

fn chord(keys: &ButtonInput<KeyCode>, key: KeyCode) -> String {
    let mut value = String::new();
    if keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]) { value.push_str("Ctrl+"); }
    if keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]) { value.push_str("Alt+"); }
    if keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]) { value.push_str("Shift+"); }
    value.push_str(&key_name(key)); value
}

pub(super) fn display_key(key: &str) -> String {
    if key.is_empty() { return "Unbound".into(); }
    key.split('+').map(|part| match part {
        "BracketLeft" => "[", "BracketRight" => "]", "Backquote" => "`",
        "Space" => "Space", "ArrowUp" => "Up", "ArrowDown" => "Down",
        "ArrowLeft" => "Left", "ArrowRight" => "Right",
        p => p.strip_prefix("Key").or_else(|| p.strip_prefix("Digit")).unwrap_or(p),
    }).collect::<Vec<_>>().join("+")
}

impl Preferences {
    pub(super) fn advanced_tooltips(&self, keys: &ButtonInput<KeyCode>, typing: bool) -> bool {
        self.advanced_tooltips || (!typing && keys.pressed(KeyCode::KeyY))
    }
    pub(crate) fn pressed(&self, action: &str, keys: &ButtonInput<KeyCode>) -> bool {
        self.bindings.get(action).filter(|v| !v.is_empty()).is_some_and(|binding|
            keys.get_just_pressed().any(|key| chord(keys, *key) == *binding))
    }
    pub(crate) fn slot_pressed(&self, keys: &ButtonInput<KeyCode>) -> Option<usize> {
        (0..17).find(|slot| self.pressed(&format!("slot{slot}"), keys))
    }
    /// The quick slot whose key is held down (a held skill key keeps attacking).
    pub(crate) fn slot_held(&self, keys: &ButtonInput<KeyCode>) -> Option<usize> {
        (0..17).find(|slot| self.bindings.get(&format!("slot{slot}")).filter(|v| !v.is_empty())
            .is_some_and(|binding| keys.get_pressed().any(|key| chord(keys, *key) == *binding)))
    }
    pub(crate) fn label(&self, action: &str) -> String {
        self.bindings.get(action).map_or_else(|| "Unbound".into(), |key| display_key(key))
    }
    pub(crate) fn slot_label(&self, slot: usize) -> String {
        let label = self.label(&format!("slot{slot}"));
        if label == "Unbound" { match slot { 0 => "LMB".into(), 1 => "RMB".into(), _ => String::new() } } else { label }
    }
    fn bind(&mut self, action: &str, value: String) -> Option<String> {
        let conflict = self.bindings.iter().find(|(id, key)| id.as_str() != action && !value.is_empty() && **key == value).map(|(id, _)| id.clone());
        if let Some(id) = &conflict { self.bindings.insert(id.clone(), String::new()); }
        self.bindings.insert(action.to_owned(), value); conflict
    }
    fn load() -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        let raw = std::fs::read_to_string("game-settings.json").ok();
        #[cfg(target_arch = "wasm32")]
        let raw = web_sys::window().and_then(|w| w.local_storage().ok().flatten()).and_then(|s| s.get_item("dsor.settings").ok().flatten());
        let mut result: Self = raw.and_then(|r| serde_json::from_str(&r).ok()).unwrap_or_default();
        result.map_transparency = result.map_transparency.min(100);
        result.graphics.sharpening = result.graphics.sharpening.min(100);
        for volume in [&mut result.master_volume, &mut result.music_volume, &mut result.ambient_volume, &mut result.effects_volume] { *volume = (*volume).min(100); }
        for id in ["slot10", "slot11", "slot12"] { result.bindings.remove(id); }
        result.migrate();
        for (id, _, key) in shortcuts() { result.bindings.entry(id).or_insert_with(|| key.into()); }
        result
    }
    /// Saved files keep every binding, so new defaults reach them only here; a
    /// binding the player changed stays as it is.
    fn migrate(&mut self) {
        // A binding missing from the file had the defaults of its time.
        let was = |p: &Self, id: &str, key: &str| p.bindings.get(id).is_none_or(|k| k == key);
        if self.keymap < 1 && was(self, "questlog", "KeyU") && was(self, "slot13", "KeyQ") {
            self.bindings.insert("questlog".into(), "KeyQ".into());
            self.bindings.insert("slot13".into(), "KeyU".into());
        }
        if self.keymap < 2 {
            let taken = |p: &Self, key: &str| p.bindings.values().any(|k| k == key);
            // The bottom-left consumable leaves E to the events (G, unless taken).
            if was(self, "slot14", "KeyE") {
                let key = if taken(self, "KeyG") { "" } else { "KeyG" };
                self.bindings.insert("slot14".into(), key.into());
            }
            // The new windows' keys, unless the player gave them to another action.
            for (id, key) in [("eventwindow", "KeyE"), ("leaderboards", "KeyL"), ("matchmakingwindow", "KeyJ")] {
                if self.bindings.contains_key(id) { continue; }
                let key = if taken(self, key) { "" } else { key };
                self.bindings.insert(id.into(), key.into());
            }
        }
        if self.keymap < 3 {
            let taken = |p: &Self, key: &str| p.bindings.values().any(|k| k == key);
            if was(self, "locationmap", "Tab") {
                let key = if taken(self, "KeyN") { "" } else { "KeyN" };
                self.bindings.insert("locationmap".into(), key.into());
            }
            if !self.bindings.contains_key("swap_attack") {
                let key = if taken(self, "Tab") { "" } else { "Tab" };
                self.bindings.insert("swap_attack".into(), key.into());
            }
        }
        self.keymap = KEYMAP;
    }
    fn save(&self) -> Result<(), String> {
        let raw = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        #[cfg(not(target_arch = "wasm32"))]
        { std::fs::write("game-settings.json.tmp", &raw).and_then(|_| std::fs::rename("game-settings.json.tmp", "game-settings.json")).map_err(|e| e.to_string()) }
        #[cfg(target_arch = "wasm32")]
        { web_sys::window().and_then(|w| w.local_storage().ok().flatten()).ok_or("Local storage is unavailable")?
            .set_item("dsor.settings", &raw).map_err(|_| "Could not save settings".into()) }
    }
}

#[derive(Resource)]
pub(crate) struct Settings {
    pub current: Preferences,
    pub(crate) dlss_availability: DlssAvailability,
    draft: Preferences,
    pub(crate) open: bool,
    tab: u8,
    pub(super) page: usize,
    listening: Option<String>,
    notice: String,
}
impl Default for Settings {
    fn default() -> Self {
        let current = Preferences::load();
        let open = std::env::var("DSOR_UI_OPEN").is_ok_and(|v| v.split(',').any(|w| w == WINDOW));
        Self { draft: current.clone(), current, dlss_availability: default(), open, tab: 0, page: 0, listening: None, notice: "Changes take effect when you choose Apply.".into() }
    }
}

impl Settings {
    /// The currency bag's pick: kept at once, as the game keeps its
    /// "/Settings/CurrencyDisplay" (no Apply).
    pub(super) fn pick_currency(&mut self, id: &str) {
        if self.current.currency == id { return; }
        self.current.currency = id.to_owned();
        self.draft.currency = id.to_owned();
        if let Err(e) = self.current.save() { warn!("Settings save failed: {e}"); }
    }
    /// A confirmation dialog's "Do not show this window again": never asked
    /// again, kept in the preferences.
    pub(super) fn skip_dialog(&mut self, id: u32) {
        if self.current.skipped_dialogs.contains(&id) { return; }
        self.current.skipped_dialogs.push(id);
        if !self.draft.skipped_dialogs.contains(&id) { self.draft.skipped_dialogs.push(id); }
        if let Err(e) = self.current.save() { warn!("Settings save failed: {e}"); }
    }
    pub(super) fn begin(&mut self) {
        self.draft = self.current.clone(); self.open = true; self.listening = None;
        self.notice = "Changes take effect when you choose Apply.".into();
    }
    pub(super) fn keyboard(&mut self, keys: &ButtonInput<KeyCode>) -> bool {
        if !self.open { return false; }
        if let Some(action) = self.listening.clone() {
            if keys.just_pressed(KeyCode::Escape) { self.listening = None; self.notice = "Binding unchanged.".into(); return true; }
            if let Some(key) = keys.get_just_pressed().copied().filter(|k| !matches!(k,
                KeyCode::ShiftLeft | KeyCode::ShiftRight | KeyCode::ControlLeft | KeyCode::ControlRight |
                KeyCode::AltLeft | KeyCode::AltRight | KeyCode::SuperLeft | KeyCode::SuperRight)).min_by_key(|k| key_name(*k)) {
                let debug_chord = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight])
                    && keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight])
                    && matches!(key, KeyCode::KeyD | KeyCode::KeyN);
                if debug_chord || matches!(key, KeyCode::Enter | KeyCode::F1 | KeyCode::F2 | KeyCode::F3 | KeyCode::F6 | KeyCode::F9 | KeyCode::F10 | KeyCode::F11 | KeyCode::F12) {
                    self.notice = "That key is reserved. Choose another key, or Esc to cancel.".into(); return true;
                }
                let binding = if matches!(key, KeyCode::Delete | KeyCode::Backspace) { String::new() } else { chord(keys, key) };
                let conflict = self.draft.bind(&action, binding);
                self.notice = conflict.map(|id| format!("Reassigned from {}.", shortcuts().iter().find(|s| s.0 == id).map_or(id.as_str(), |s| s.1.as_str())))
                    .unwrap_or_else(|| "Binding updated. Choose Apply to save.".into());
                self.listening = None;
            }
            return true;
        }
        if keys.just_pressed(KeyCode::Escape) { self.open = false; }
        self.open
    }
    pub(super) fn action(&mut self, event: &str) {
        match event {
            "general" | "graphics" | "audio" | "controls" | "cursor" | "combatSettings" => {
                self.tab = ["general", "graphics", "audio", "controls", "cursor", "combatSettings"].iter().position(|t| *t == event).unwrap() as u8;
                self.page = 0; self.listening = None;
            }
            "cancel" => { self.open = false; self.listening = None; }
            "apply" => match self.draft.save() {
                Ok(()) => { self.current = self.draft.clone(); self.open = false; self.listening = None; }
                Err(e) => { warn!("Settings save failed: {e}"); self.notice = "Could not save settings. Your changes are still here; try Apply again.".into(); }
            },
            "revert" => { self.draft = Preferences::default(); self.listening = None; self.notice = "Defaults restored in this draft. Apply to save, or Cancel to keep your settings.".into(); }
            "previous" => { self.page = self.page.saturating_sub(1); self.listening = None; }
            "next" => { self.page = (self.page + 1).min(self.rows().len().saturating_sub(1) / ROWS); self.listening = None; }
            _ => if let Some(row) = event.strip_prefix("row").and_then(|s| s.parse::<usize>().ok()) {
                if let Some((id, _, _)) = self.rows().get(self.page * ROWS + row).cloned() {
                    match id.as_str() {
                        "tooltips" => self.draft.tooltips ^= true, "own_name" => self.draft.own_name ^= true,
                        "advanced_tooltips" => self.draft.advanced_tooltips ^= true,
                        "other_names" => self.draft.other_names ^= true, "combat_text" => self.draft.combat_text ^= true,
                        "camera_shake" => self.draft.camera_shake ^= true, "attack_hovered" => self.draft.attack_hovered ^= true,
                        "monster_effects" => self.draft.monster_effects ^= true,
                        "classic_cursor" => self.draft.classic_cursor ^= true,
                        "return_link" => self.draft.return_link ^= true, "return_checkin" => self.draft.return_checkin ^= true,
                        "lock_bar" => self.draft.lock_bar ^= true, "fullscreen" => self.draft.fullscreen ^= true,
                        "vsync" => self.draft.vsync ^= true,
                        "audio_muted" => self.draft.audio_muted ^= true,
                        _ if self.tab == 1 => self.draft.graphics.action(&id, self.dlss_availability),
                        _ if self.tab == 3 => { self.listening = Some(id); self.notice = "Press a key (modifiers supported). Delete clears; Esc cancels.".into(); }
                        _ => {}
                    }
                }
            }
        }
    }
    pub(super) fn rows(&self) -> Vec<(String, String, String)> {
        let on = |v| if v { "On" } else { "Off" }.to_owned();
        let p = &self.draft;
        let rows: Vec<(&str, &str, String)> = match self.tab {
            0 => vec![("tooltips", "Hover descriptions", on(p.tooltips)),
                ("advanced_tooltips", "Always show advanced tooltips", on(p.advanced_tooltips)), ("own_name", "Display your name", on(p.own_name)),
                ("other_names", "Display other names", on(p.other_names)), ("map_transparency", "Map transparency", format!("{}%", p.map_transparency)),
                ("return_link", "Show the Return Link button", on(p.return_link)), ("return_checkin", "Show the Return Check-in button", on(p.return_checkin))],
            1 => {
                let mut rows = vec![("fullscreen", "Fullscreen", on(p.fullscreen)), ("vsync", "Vertical sync", on(p.vsync))];
                rows.extend(p.graphics.rows(self.dlss_availability));
                rows.push(("map_transparency", "Map transparency", format!("{}%", p.map_transparency)));
                rows
            },
            2 => vec![("audio_muted", "Mute all audio", on(p.audio_muted)),
                ("master_volume", "Master volume", format!("{}%",p.master_volume)),
                ("music_volume", "Music volume", format!("{}%",p.music_volume)),
                ("ambient_volume", "Ambience volume", format!("{}%",p.ambient_volume)),
                ("effects_volume", "Effects volume", format!("{}%",p.effects_volume))],
            3 => return shortcuts().into_iter().map(|(id, title, _)| {
                let label = if self.listening.as_ref() == Some(&id) { "Press a key...".into() } else { p.label(&id) };
                (id, title, label)
            }).collect(),
            4 => vec![("classic_cursor", "Classic cursor", on(p.classic_cursor)),
                ("attack_hovered", "Left click attacks hovered enemies", on(p.attack_hovered)),
                ("fixed", "Hold Shift + left click", "Attack in place".into()), ("fixed", "Hold right click", "Secondary attack".into())],
            _ => vec![("combat_text", "Floating combat text", on(p.combat_text)), ("camera_shake", "Camera shake", on(p.camera_shake)),
                ("lock_bar", "Lock action bar", on(p.lock_bar)), ("monster_effects", "Status effects on enemies", on(p.monster_effects))],
        };
        rows.into_iter().map(|(id, label, v)| (id.into(), label.into(), v)).collect()
    }

    fn percent(&self, row: usize) -> Option<(String, u8)> {
        let id = self.rows().get(self.page * ROWS + row)?.0.clone();
        let value = match id.as_str() {
            "map_transparency" => self.draft.map_transparency,
            "sharpening" => self.draft.graphics.sharpening,
            "master_volume" => self.draft.master_volume,
            "music_volume" => self.draft.music_volume,
            "ambient_volume" => self.draft.ambient_volume,
            "effects_volume" => self.draft.effects_volume,
            _ => return None,
        };
        Some((id, value))
    }

    fn set_percent(&mut self, id: &str, value: u8) {
        let target = match id {
            "map_transparency" => &mut self.draft.map_transparency,
            "sharpening" => &mut self.draft.graphics.sharpening,
            "master_volume" => &mut self.draft.master_volume,
            "music_volume" => &mut self.draft.music_volume,
            "ambient_volume" => &mut self.draft.ambient_volume,
            "effects_volume" => &mut self.draft.effects_volume,
            _ => return,
        };
        *target = value.min(100);
    }
}

fn track(rect: [f32; 4]) -> (f32, f32) {
    let scale = (rect[2] - rect[0]) / 190.0;
    (rect[0] + 8.0 * scale, rect[2] - 48.0 * scale)
}

pub(super) struct SliderDrag { root: Entity, index: usize, row: usize, id: String, grab: f32 }

/// A press owns its slider until release, even outside the row or window.
pub(super) fn drag_slider(pointer: &mut Pointer, windows: &Windows, mouse: &ButtonInput<MouseButton>,
    point: Option<Vec2>, width: f32, allowed: bool, settings: &mut Settings) -> bool {
    if !allowed || !settings.open { pointer.slider = None; return false; }
    if mouse.just_pressed(MouseButton::Left) {
        pointer.slider = None;
        if let Some((root, index)) = pointer.down {
            if let Ok((_, name, controls, _, tf)) = windows.get(root) {
                let b = &controls.buttons[index];
                if let Action::Settings(event) = &b.action {
                    if let Some(row) = event.strip_prefix("row").and_then(|s| s.parse::<usize>().ok()) {
                        if let Some((id, value)) = settings.percent(row).filter(|_| name.0 == WINDOW) {
                            if let Some(p) = point {
                                let (left, right) = track(b.rect);
                                let x = p.x - window_offset(tf).x - b.k * (width - BASE_W);
                                if x >= left - 6.0 && x <= right + 6.0 {
                                    let grab = crate::ui::slider::grab_offset(x, left, right, value as f32 / 100.0, 8.0);
                                    pointer.slider = Some(SliderDrag { root, index, row, id, grab });
                                }
                            }
                            // The value label is not an increment button.
                            if pointer.slider.is_none() { pointer.down = None; }
                        }
                    }
                }
            }
        }
    }
    let Some(drag) = pointer.slider.as_ref() else { return false; };
    let valid = windows.get(drag.root).ok().filter(|(_, _, _, v, _)| **v != Visibility::Hidden)
        .filter(|_| settings.percent(drag.row).is_some_and(|(id, _)| id == drag.id));
    if let Some((_, _, controls, _, tf)) = valid {
        if (mouse.pressed(MouseButton::Left) || mouse.just_released(MouseButton::Left)) && point.is_some() {
            let b = &controls.buttons[drag.index];
            let (left, right) = track(b.rect);
            let x = point.unwrap().x - window_offset(tf).x - b.k * (width - BASE_W) - drag.grab;
            settings.set_percent(&drag.id, crate::ui::slider::value_at(x, left, right, 0, 100));
        }
    } else { pointer.slider = None; pointer.down = None; return true; }
    if !mouse.pressed(MouseButton::Left) { pointer.slider = None; pointer.down = None; }
    true
}

pub(in crate::ui) fn row_index(w: &Widget) -> Option<usize> {
    w.id.strip_prefix("Canvas/Runtime/row")?.parse().ok()
}

#[derive(Component)]
pub(in crate::ui) struct RowArt {
    pub row: usize, pub rect: [f32; 4], pub original: Art, pub depth: f32,
    pub shown: Option<Option<u8>>,
}

pub(in crate::ui) fn sliders(settings: Res<Settings>, mut rows: Query<(&mut RowArt, &Mesh2d)>, mut meshes: ResMut<Assets<Mesh>>) {
    for (mut row, mesh) in &mut rows {
        let value = settings.percent(row.row).map(|(_, value)| value);
        if row.shown == Some(value) { continue; }
        let mut run = Run::default();
        if let Some(value) = value {
            let (left, right) = track(row.rect);
            let y = (row.rect[1] + row.rect[3]) * 0.5;
            let x = left + (right - left) * value as f32 / 100.0;
            let mut quad = |l, t, r, b, color: [f32; 4]| run.push(&Art { tex: None,
                pos: vec![l,t,r,t,r,b,l,b], uv: vec![0.;8], color: color.repeat(4), idx: vec![0,1,2,0,2,3], saturation: 1.0 }, row.depth);
            quad(left, y-3., right, y+3., [0.24,0.19,0.12,1.]);
            quad(left, y-2., x, y+2., [0.73,0.52,0.23,1.]);
            quad(x-6., y-10., x+6., y+10., [0.86,0.69,0.40,1.]);
            quad(x-4., y-8., x+4., y+8., [0.42,0.29,0.13,1.]);
        } else { run.push(&row.original, row.depth); }
        if let Some(mut mesh) = meshes.get_mut(&mesh.0) {
            let mut updated = run.mesh();
            // Slider vertices change after upload, so retain their CPU copy.
            updated.asset_usage |= RenderAssetUsages::MAIN_WORLD;
            *mesh = updated;
        }
        row.shown = Some(value);
    }
}

#[derive(Component)]
pub(in crate::ui) struct Caption(pub String);

/// Keep the original frame, tabs and footer; populate its list with real rows.
pub(in crate::ui) fn prepare(layout: &Layout) -> Layout {
    let mut out = layout.clone();
    out.widgets.retain(|w| !w.id.contains("/ContentList") && !w.id.contains("/Scroll") && !w.id.contains("/DummyScroll"));
    for w in &mut out.widgets {
        if w.id.ends_with("/TitleBarLabel") { if let Some(t) = &mut w.text { t.text = "Game Settings".into(); } }
    }
    for row in 0..ROWS {
        let y = 216.0 + row as f32 * 38.0;
        add_widget(&mut out, &format!("row{row}"), [540.0, y, 730.0, y+32.0], true, "");
        add_widget(&mut out, &format!("label{row}"), [290.0, y, 535.0, y+32.0], false, "");
    }
    add_widget(&mut out, "notice", [290.0, 485.0, 735.0, 509.0], false, "");
    add_widget(&mut out, "previous", [290.0, 512.0, 360.0, 537.0], true, "Previous");
    add_widget(&mut out, "page", [370.0, 512.0, 650.0, 537.0], false, "");
    add_widget(&mut out, "next", [660.0, 512.0, 730.0, 537.0], true, "Next");
    // A larger modal improves readability without changing the HUD scale.
    for w in &mut out.widgets {
        for (i,v) in w.rect.iter_mut().enumerate() { let center = if i%2 == 0 { 512.0 } else { 384.0 }; *v = center+(*v-center)*1.2; }
        if let Some(art) = &mut w.art { for (i,v) in art.pos.iter_mut().enumerate() { let center = if i%2 == 0 { 512.0 } else { 384.0 }; *v = center+(*v-center)*1.2; } }
        if let Some(label) = &mut w.text { label.size *= 1.15; }
    }
    out
}

fn add_widget(layout: &mut Layout, id: &str, rect: [f32;4], button: bool, value: &str) {
    let [l,t,r,b] = rect;
    let art = button.then(|| Art { tex: None, pos: vec![l,t,r,t,r,b,l,b,l+1.0,t+1.0,r-1.0,t+1.0,r-1.0,b-1.0,l+1.0,b-1.0], uv: vec![0.0;16],
        color: [[0.45,0.33,0.16,1.0].repeat(4),[0.12,0.085,0.045,1.0].repeat(4)].concat(), idx: vec![0,1,2,0,2,3,4,5,6,4,6,7], saturation: 1.0 });
    layout.widgets.push(Widget { id: format!("Canvas/Runtime/{id}"), kind: if button { "Button" } else { "TextLabel" }.into(),
        event: id.into(), click_through: !button, enabled: true, button: None, state: None, spell: None,
        k: 0.5, rect, hidden: false, art, text: Some(Label { text: value.into(), key: "gui.settings.runtime".into(),
            size: if id == "notice" { 8.0 } else { 10.0 }, color: [0.9,0.78,0.54,1.0], align: [if button || id == "page" { 1 } else { 0 },1], bold: false, word_break: true, outline: Vec::new() }) });
}

pub(in crate::ui) fn sync(settings: ResMut<Settings>, mut windows: Query<(&Window2018, &mut WindowControls, &mut Visibility)>,
    mut captions: Query<(&Caption, &mut Text2d, &mut Transform, &mut Anchor, &mut bevy::text::TextBounds, &mut TextLayout)>, mut primary: Query<&mut Window, With<PrimaryWindow>>, mut map: ResMut<local_map::MapSettings>,
    screen: Res<Screen>,
    mut applied: Local<Option<Preferences>>) {
    let shown = windows.iter().any(|(w,_,v)| w.0 == WINDOW && *v != Visibility::Hidden);
    // Also support opening the settings window directly in screenshot fixtures.
    if shown && !settings.open { for (w,_,mut v) in &mut windows { if w.0 == WINDOW { *v = Visibility::Hidden; } } }
    if applied.as_ref() != Some(&settings.current) {
        if let Ok(mut window) = primary.single_mut() {
            if applied.as_ref().is_none_or(|p| p.fullscreen != settings.current.fullscreen) {
                window.mode = if settings.current.fullscreen { WindowMode::BorderlessFullscreen(MonitorSelection::Current) } else { WindowMode::Windowed };
            }
            window.present_mode = if settings.current.vsync { bevy::window::PresentMode::AutoVsync } else { bevy::window::PresentMode::AutoNoVsync };
        }
        let delta = settings.current.map_transparency as i8 - map.transparency() as i8;
        map.adjust(delta);
        for (w,mut c,_) in &mut windows { if w.0 == "bottombar" { c.locked = settings.current.lock_bar; } }
        *applied = Some(settings.current.clone());
    }
    // The game bar's optional buttons (page 1 hides them), whenever it is built.
    let hidden = [!settings.current.return_link, !settings.current.return_checkin];
    for (w,mut c,_) in &mut windows {
        if w.0 != "gamebar" { continue; }
        for (group, hide) in GAMEBAR_OPTIONAL.into_iter().zip(hidden) {
            if c.pages.get(group).copied().unwrap_or(0) != u8::from(hide) { c.pages.insert(group, u8::from(hide)); }
        }
    }
    let rows = settings.rows();
    for (w,mut c,_) in &mut windows {
        if w.0 != WINDOW { continue; }
        c.pages.insert("settings", settings.tab);
        c.pages.insert("settings_rows", rows.len().saturating_sub(settings.page*ROWS).min(ROWS) as u8);
        for b in &mut c.buttons {
            let leaf = b.id.rsplit('/').next().unwrap_or("");
            if let Some(row) = leaf.strip_prefix("row").and_then(|s| s.parse::<usize>().ok()) {
                b.authored_enabled = rows.get(settings.page*ROWS+row).is_some_and(|r| !matches!(r.0.as_str(), "unavailable" | "fixed"));
            } else if leaf == "previous" { b.authored_enabled = settings.page > 0; }
            else if leaf == "next" { b.authored_enabled = (settings.page+1)*ROWS < rows.len(); }
        }
    }
    for (caption,mut text,mut tf,mut anchor,mut bounds,mut layout) in &mut captions {
        let id = caption.0.as_str();
        let row = id.strip_prefix("row").or_else(|| id.strip_prefix("label")).and_then(|s| s.parse::<usize>().ok());
        if let Some(row) = row.filter(|_| id.starts_with("row")) {
            if let Some(b) = windows.iter().filter(|(w,_,_)| w.0 == WINDOW)
                .flat_map(|(_,c,_)| c.buttons.iter()).find(|b| b.action == Action::Settings(format!("row{row}"))) {
                let slider = settings.percent(row).is_some();
                tf.translation.x = if slider { b.rect[2]-4.0 } else { (b.rect[0]+b.rect[2])*0.5 };
                anchor.0.x = if slider { 0.5 } else { 0.0 };
                let width = (b.rect[2]-b.rect[0]) * if slider { 0.22 } else { 1.0 } * screen.scale;
                if bounds.width != Some(width) { *bounds = bevy::text::TextBounds::new_horizontal(width); }
                let justify = if slider { Justify::Right } else { Justify::Center };
                if layout.justify != justify { layout.justify = justify; }
            }
        }
        let value = if let Some(row) = row { rows.get(settings.page*ROWS+row).map(|r| if id.starts_with("label") { r.1.clone() } else { r.2.clone() }).unwrap_or_default() }
        else if id == "page" { format!("{}  |  {} / {}", ["General", "Graphics", "Audio", "Hotkeys", "Mouse", "Combat"][settings.tab as usize], settings.page+1, rows.len().max(1).div_ceil(ROWS)) }
        else if id == "notice" { settings.notice.clone() }
        else { continue; };
        if text.0 != value { text.0 = value; }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn graphics_pages_edit_only_the_draft_and_reset_all_options() {
        // As Preferences::load reads an old file.
        let mut p: Preferences = serde_json::from_str(r#"{"vsync":true}"#).unwrap();
        p.migrate();
        assert_eq!(p.graphics, GraphicsSettings::default());
        let mut s = Settings { dlss_availability: default(), current:p.clone(), draft:p.clone(), open:true, tab:1, page:0, listening:None, notice:String::new() };
        let click = |s: &mut Settings, id: &str| {
            let index = s.rows().iter().position(|row| row.0 == id).unwrap();
            s.page = index / ROWS;
            s.action(&format!("row{}", index % ROWS));
        };
        assert_eq!(s.rows().len().div_ceil(ROWS), 2);
        click(&mut s, "anti_aliasing");
        click(&mut s, "shadows");
        click(&mut s, "bloom");
        click(&mut s, "vignette");
        s.set_percent("sharpening", 62);
        assert!(s.current == p);
        assert!(!s.draft.graphics.bloom);
        assert!(s.draft.graphics.vignette, "off by default, so one click turns it on");
        assert_eq!(s.draft.graphics.sharpening, 62);
        let edited = s.draft.clone();
        click(&mut s, "unavailable");
        assert!(s.draft == edited);
        s.action("cancel"); s.begin();
        assert!(s.draft == p);
        s.current = edited; s.begin(); s.action("revert");
        assert!(s.draft == p);
        assert!(!s.current.graphics.bloom);
    }

    #[test]
    fn slider_art_stays_above_frame_and_editable_after_upload() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()))
            .init_asset::<Layout>().init_asset::<Mesh>().init_asset::<UiMaterial>()
            .init_asset::<Image>().init_asset::<Font>().init_asset::<dialogue::Catalog>()
            .init_resource::<Loaded>().init_resource::<Screen>().init_resource::<dialogue::Presentation>().init_resource::<super::party::Party>().init_resource::<super::quests::Quests>().init_resource::<crate::gameplay::quests::QuestUi>().init_resource::<super::systems::Views>().init_resource::<super::currency::Shown>().init_resource::<super::message::ItemMessages>().init_resource::<crate::ui::UiWork>()
            .init_resource::<Settings>().add_systems(Update, (spawn_windows, sliders).chain());
        let handle = app.world_mut().resource_mut::<Assets<Layout>>().add(Layout { window: WINDOW.into(), widgets: Vec::new() });
        app.world_mut().resource_mut::<Loaded>().layouts.push((handle, 100., None));
        app.world_mut().resource_mut::<Loaded>().spawned.push(false);
        app.update();
        let root = app.world_mut().query_filtered::<Entity,With<Window2018>>().single(app.world()).unwrap();
        *app.world_mut().get_mut::<Visibility>(root).unwrap() = Visibility::Inherited;
        app.update();
        let (mesh, rect) = app.world_mut().query::<(&RowArt, &Mesh2d, &Transform)>().iter(app.world())
            .find(|(row,_,_)| row.row == 1).map(|(row,mesh,tf)| {
                assert!(tf.translation.z > 0.);
                (mesh.0.clone(), row.rect)
            }).unwrap();
        assert_eq!(app.world().resource::<Assets<Mesh>>().get(&mesh).unwrap().count_vertices(), 8);
        app.world_mut().resource_mut::<Settings>().action("audio");
        app.world_mut().resource_mut::<Settings>().set_percent("master_volume",37);
        app.update();
        let assets = app.world().resource::<Assets<Mesh>>();
        let mesh = assets.get(&mesh).unwrap();
        assert!(mesh.asset_usage.contains(RenderAssetUsages::MAIN_WORLD));
        assert!(mesh.asset_usage.contains(RenderAssetUsages::RENDER_WORLD));
        assert_eq!(mesh.count_vertices(), 16);
        let bevy::mesh::VertexAttributeValues::Float32x3(points) = mesh.attribute(Mesh::ATTRIBUTE_POSITION).unwrap() else { panic!("missing slider geometry") };
        let (left,right) = track(rect);
        assert!((points[5][0] - (left+(right-left)*0.37)).abs() < 0.001);
    }

    #[test]
    fn percentage_rows_use_drafts_and_never_cycle_on_click() {
        let p = Preferences::default();
        let mut s = Settings { dlss_availability: default(), current:p.clone(), draft:p.clone(), open:true, tab:0, page:0, listening:None, notice:String::new() };
        for tab in ["general", "graphics", "audio"] {
            s.action(tab);
            for row in 0..s.rows().len() {
                if let Some((id, _)) = s.percent(row) {
                    s.set_percent(&id, 37);
                    s.action(&format!("row{row}"));
                    assert_eq!(s.percent(row).unwrap().1, 37);
                }
            }
        }
        assert!(s.current == p);
        s.action("cancel"); s.begin();
        assert!(s.draft == p);
    }

    #[test]
    fn slider_drag_tracks_scaled_moved_window_and_stays_captured_outside_row() {
        let mut app = App::new();
        let p = Preferences::default();
        app.insert_resource(Settings { dlss_availability: default(), current:p.clone(), draft:p, open:true, tab:2, page:0, listening:None, notice:String::new() })
            .init_resource::<ButtonInput<MouseButton>>().init_resource::<Pointer>()
            .init_resource::<SkillClicks>().init_resource::<crate::network::net::RespawnChoice>()
            .init_resource::<crate::network::chat::ChatLog>().init_resource::<crate::gameplay::dialogue::Dialogue>()
            .init_resource::<social::SocialUi>().init_resource::<hotbar::Hotbar>().init_resource::<crate::ui::controls::hotbar::ItemCooldowns>().init_resource::<crate::gameplay::quests::QuestUi>().init_resource::<crate::ui::controls::systems::Views>()
            .init_resource::<super::party::Party>()
            .add_message::<bevy::input::mouse::MouseWheel>().add_systems(Update, interact);
        let primary = app.world_mut().spawn((Window { focused: true, resolution: (1600,900).into(), ..default() }, PrimaryWindow)).id();
        let layout = prepare(&Layout { window: WINDOW.into(), widgets: Vec::new() });
        let mut controls = WindowControls::new(&layout);
        for w in &layout.widgets { controls.region(w.k, w.rect, controls.gate(WINDOW, w)); }
        let b = controls.buttons.iter().find(|b| b.action == Action::Settings("row1".into())).unwrap();
        let (left,right) = track(b.rect);
        let y = (b.rect[1]+b.rect[3])*0.5;
        let extra = b.k * (BASE_H*1600.0/900.0 - BASE_W);
        app.world_mut().spawn((Window2018(WINDOW.into()), controls, Visibility::Inherited, Transform::from_xyz(70., -35., 200.)));
        let cursor = |app: &mut App, x, y| {
            app.world_mut().get_mut::<Window>(primary).unwrap().set_cursor_position(Some(Vec2::new(x+70.+extra,y+35.)*(900.0/BASE_H)));
        };
        cursor(&mut app, left+(right-left)*0.37, y);
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        app.update();
        assert_eq!(app.world().resource::<Settings>().draft.master_volume, 37);
        assert_eq!(app.world().resource::<Settings>().current.master_volume, 80);
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
        cursor(&mut app, right+300., y+200.);
        app.update();
        assert_eq!(app.world().resource::<Settings>().draft.master_volume, 100);
        cursor(&mut app, left-300., y-200.);
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
        app.update();
        assert_eq!(app.world().resource::<Settings>().draft.master_volume, 0);
        assert!(app.world().resource::<Pointer>().slider.is_none());
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
        // A press outside the row cannot pick up a slider midway through a hold.
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        app.update();
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
        cursor(&mut app, right, y);
        app.update();
        assert_eq!(app.world().resource::<Settings>().draft.master_volume, 0);
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().reset_all();
        cursor(&mut app, left+(right-left)*0.5, y);
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        app.update();
        assert_eq!(app.world().resource::<Settings>().draft.master_volume, 50);
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
        app.world_mut().get_mut::<Window>(primary).unwrap().focused = false;
        cursor(&mut app, right, y);
        app.update();
        assert_eq!(app.world().resource::<Settings>().draft.master_volume, 50);
        assert!(app.world().resource::<Pointer>().slider.is_none());
    }

    #[test]
    fn classic_cursor_is_optional_persistent_and_uses_the_settings_draft() {
        // Settings written before the cursor option retain the current style.
        let p: Preferences = serde_json::from_str(r#"{"tooltips":true}"#).unwrap();
        assert!(!p.classic_cursor);
        let mut s = Settings { dlss_availability: default(), current:p.clone(), draft:p, open:false, tab:0, page:0, listening:None, notice:String::new() };
        s.begin(); s.action("cursor");
        let row = s.rows().iter().position(|r| r.0 == "classic_cursor").unwrap();
        assert_eq!(s.rows()[row].2, "Off");
        s.action(&format!("row{row}"));
        assert_eq!(s.rows()[row].2, "On");
        assert!(!s.current.classic_cursor, "draft changes wait for Apply");
        let restored: Preferences = serde_json::from_str(&serde_json::to_string(&s.draft).unwrap()).unwrap();
        assert!(restored.classic_cursor);
        s.action("cancel"); s.begin();
        assert!(!s.draft.classic_cursor, "Cancel discards the change");
        s.current = restored;
        s.begin(); s.action("revert");
        assert!(!s.draft.classic_cursor, "Reset restores the modern cursor in the draft");
        assert!(s.current.classic_cursor, "Reset must also wait for Apply");
    }
    #[test]
    fn advanced_tooltips_persist_and_keep_the_temporary_key_override() {
        let mut p: Preferences = serde_json::from_str(r#"{"tooltips":true}"#).unwrap();
        let mut keys = ButtonInput::default();
        assert!(!p.advanced_tooltips(&keys,false));
        keys.press(KeyCode::KeyY);
        assert!(p.advanced_tooltips(&keys,false));
        assert!(!p.advanced_tooltips(&keys,true));
        p.advanced_tooltips = true;
        let p:Preferences=serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
        keys.release(KeyCode::KeyY);
        assert!(p.advanced_tooltips(&keys,false));
        assert!(p.advanced_tooltips(&keys,true));
        let mut s = Settings {dlss_availability: default(), current:p.clone(),draft:p,open:false,tab:0,page:0,listening:None,notice:String::new()};
        s.begin();
        let row=s.rows().iter().position(|r|r.0=="advanced_tooltips").unwrap();
        s.action(&format!("row{row}"));
        assert!(!s.draft.advanced_tooltips);
        s.action("cancel");
        assert!(s.current.advanced_tooltips);
        s.begin(); assert!(s.draft.advanced_tooltips);
    }
    #[test]
    fn rebind_replaces_conflicts_and_drives_gameplay() {
        let mut p = Preferences::default();
        assert_eq!(p.bind("slot2", "KeyI".into()), Some("inventory".into()));
        let mut keys = ButtonInput::default(); keys.press(KeyCode::KeyI);
        assert_eq!(p.slot_pressed(&keys), Some(2)); assert!(!p.pressed("inventory", &keys));
        assert_eq!(p.slot_label(2), "I");
        keys.press(KeyCode::ControlLeft); assert_eq!(p.slot_pressed(&keys), None);
        p.bind("slot2", "Ctrl+KeyI".into()); assert_eq!(p.slot_pressed(&keys), Some(2));
    }
    #[test]
    fn saved_default_bindings_move_quests_to_q_once() {
        let parse = |raw: &str| { let mut p: Preferences = serde_json::from_str(raw).unwrap(); p.migrate(); p };
        let old = parse(r#"{"bindings":{"questlog":"KeyU","slot13":"KeyQ","inventory":"KeyI"}}"#);
        assert_eq!((old.label("questlog"), old.label("slot13"), old.label("inventory"), old.keymap), ("Q".into(), "U".into(), "I".into(), KEYMAP));
        let chosen = parse(r#"{"bindings":{"questlog":"KeyJ","slot13":"KeyQ"}}"#);
        assert_eq!((chosen.label("questlog"), chosen.label("slot13")), ("J".into(), "Q".into()), "a player's own key stays");
        // Saved after the move, the old keys are the player's choice.
        let mut again = old.clone();
        again.bindings.insert("questlog".into(), "KeyU".into()); again.bindings.insert("slot13".into(), "KeyQ".into());
        let again = parse(&serde_json::to_string(&again).unwrap());
        assert_eq!((again.label("questlog"), again.label("slot13")), ("U".into(), "Q".into()));
        assert_eq!(Preferences::default().keymap, KEYMAP);
    }
    #[test]
    fn tab_swaps_the_attacks_and_the_map_overlay_moves_to_n() {
        let parse = |raw: &str| { let mut p: Preferences = serde_json::from_str(raw).unwrap(); p.migrate(); p };
        let old = parse(r#"{"keymap":2,"bindings":{"locationmap":"Tab"}}"#);
        assert_eq!((old.label("locationmap"), old.label("swap_attack")), ("N".into(), "Tab".into()));
        // A map overlay the player moved elsewhere stays there.
        let mine = parse(r#"{"keymap":2,"bindings":{"locationmap":"KeyB"}}"#);
        assert_eq!((mine.label("locationmap"), mine.label("swap_attack")), ("B".into(), "Tab".into()));
    }
    #[test]
    fn events_rankings_and_battlegrounds_take_e_l_and_j_once() {
        let parse = |raw: &str| { let mut p: Preferences = serde_json::from_str(raw).unwrap(); p.migrate(); p };
        // Saved at revision 1: the consumable on E moves to G; the windows take E, L and J.
        let old = parse(r#"{"keymap":1,"bindings":{"slot14":"KeyE","questlog":"KeyQ"}}"#);
        assert_eq!((old.label("slot14"), old.label("eventwindow"), old.label("leaderboards"), old.label("matchmakingwindow")),
            ("G".into(), "E".into(), "L".into(), "J".into()));
        // A key the player gave to another action stays theirs.
        let mine = parse(r#"{"keymap":1,"bindings":{"slot14":"KeyR","questlog":"KeyL","slot15":"KeyG"}}"#);
        assert_eq!((mine.label("slot14"), mine.label("questlog"), mine.label("leaderboards"), mine.label("eventwindow")),
            ("R".into(), "L".into(), "Unbound".into(), "E".into()));
        let mut keys = ButtonInput::default(); keys.press(KeyCode::KeyE);
        assert!(old.pressed("eventwindow", &keys) && old.slot_pressed(&keys).is_none(), "E opens the events only");
        let defaults = Preferences::default();
        assert_eq!((defaults.label("eventwindow"), defaults.label("leaderboards"), defaults.label("matchmakingwindow"), defaults.label("slot14")),
            ("E".into(), "L".into(), "J".into(), "G".into()));
        // The game bar's two comeback buttons: shown by default, rows of the
        // General tab's first page, edited in the draft until Apply.
        assert!(defaults.return_link && defaults.return_checkin);
        let mut s = Settings { dlss_availability: default(), current: defaults.clone(), draft: defaults, open: true, tab: 0, page: 0, listening: None, notice: String::new() };
        for id in ["return_link", "return_checkin"] {
            let row = s.rows().iter().position(|r| r.0 == id).unwrap();
            assert!(row < ROWS);
            s.action(&format!("row{row}"));
        }
        assert!(!s.draft.return_link && !s.draft.return_checkin && s.current.return_link);
        let saved: Preferences = serde_json::from_str(&serde_json::to_string(&s.draft).unwrap()).unwrap();
        assert!(!saved.return_link && !saved.return_checkin);
        // Each button (and its states) is its own page group: page 1 (sync) hides it.
        let mut controls = WindowControls::new(&Layout { window: "gamebar".into(), widgets: Vec::new() });
        for group in GAMEBAR_OPTIONAL {
            let section = super::super::section("gamebar", &format!("Canvas/Label/{group}/mouseover/glow20"));
            assert_eq!(section, Section::Page(group, 1));
            assert!(controls.section_visible(section));
            controls.pages.insert(group, 1);
            assert!(!controls.section_visible(section));
        }
        assert_eq!(super::super::section("gamebar", "Canvas/Label/OptionsBtn"), Section::Always);
    }
    #[test]
    fn cancel_and_reset_never_change_applied_preferences() {
        let p = Preferences::default();
        let mut s = Settings { dlss_availability: default(), current:p.clone(), draft:p.clone(), open:false, tab:0, page:0, listening:None, notice:String::new() };
        s.begin(); s.action("row0"); assert!(!s.draft.tooltips); s.action("cancel"); assert!(s.current.tooltips);
        s.begin(); assert!(s.draft.tooltips); s.action("revert"); assert!(s.open); assert_eq!(s.current.bindings, p.bindings);
        let raw = serde_json::to_string(&s.current).unwrap(); assert_eq!(serde_json::from_str::<Preferences>(&raw).unwrap().bindings, p.bindings);
    }
    #[test]
    fn monster_status_effects_are_shown_unless_turned_off_on_the_combat_tab() {
        // Files saved before the option keep the effects shown.
        let p: Preferences = serde_json::from_str(r#"{"combat_text":true}"#).unwrap();
        assert!(p.monster_effects && Preferences::default().monster_effects);
        let mut s = Settings { dlss_availability: default(), current: p.clone(), draft: p, open: true, tab: 0, page: 0, listening: None, notice: String::new() };
        s.action("combatSettings");
        let row = s.rows().iter().position(|r| r.0 == "monster_effects").unwrap();
        assert!(row < ROWS && s.rows()[row].2 == "On");
        s.action(&format!("row{row}"));
        assert!(!s.draft.monster_effects && s.current.monster_effects, "edited in the draft until Apply");
        let saved: Preferences = serde_json::from_str(&serde_json::to_string(&s.draft).unwrap()).unwrap();
        assert!(!saved.monster_effects);
        s.action("revert");
        assert!(s.draft.monster_effects);
    }
}
