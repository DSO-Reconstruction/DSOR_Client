//! Persistent lighting profiles and partial overrides (the Lighting Menu's data).
//!
//! Original ambience assets remain the baseline. A missing override means
//! "keep the original value"; it does not mean zero or a default colour.
//! crate::rendering::lighting resolves a bubble through these before it fades to
//! it, so the zones' cross-fades (entering buildings) keep working.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::rendering::lighting::{Bubble, Lighting, LocalBubble};

const SCHEMA_VERSION: u32 = 1;
const NATIVE_STORE_FILE: &str = "lighting-presets.json";
#[cfg(target_arch = "wasm32")]
const WEB_STORAGE_KEY: &str = "dsor.lighting-presets.v1";

/// The editor's working overrides and named snapshots.
#[derive(Resource, Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct LightingMenuStore {
    pub schema_version: u32,
    pub active_preset_id: Option<String>,
    pub next_preset_id: u64,
    /// Unsaved or currently edited overrides, keyed by map asset name.
    pub working_map_overrides: BTreeMap<String, MapLightingOverrides>,
    /// Stable internal IDs keep presets usable even after being renamed.
    pub presets: BTreeMap<String, LightingPreset>,
    /// Not saved. "Preview selected zone": Some(Some(i)) shows bubble i, Some(None)
    /// the global bubble, wherever the observer is; None follows the observer.
    #[serde(skip)]
    pub preview_target: Option<Option<usize>>,
}

impl Default for LightingMenuStore {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            active_preset_id: None,
            next_preset_id: 1,
            working_map_overrides: BTreeMap::new(),
            presets: BTreeMap::new(),
            preview_target: None,
        }
    }
}

/// Snapshot of the global F2 multipliers (crate::rendering::lighting::Lighting).
/// Files from the 2018 menu may carry exposure, bloom, saturation, vignette
/// and tonemapping: release261's pipeline has no such globals, so they are ignored.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct LightingControls {
    pub sun: f32,
    pub back: f32,
    pub ambient: f32,
    pub lights: f32,
    pub fog: f32,
    pub fog_range: f32,
    pub shadows: bool,
}

impl Default for LightingControls {
    fn default() -> Self { Self::from(&Lighting::default()) }
}

impl From<&Lighting> for LightingControls {
    fn from(l: &Lighting) -> Self {
        Self { sun: l.sun, back: l.back, ambient: l.ambient, lights: l.lights, fog: l.fog, fog_range: l.fog_range, shadows: l.shadows }
    }
}

impl LightingControls {
    pub fn apply(&self, l: &mut Lighting) {
        (l.sun, l.back, l.ambient, l.lights, l.fog, l.fog_range, l.shadows) =
            (self.sun, self.back, self.ambient, self.lights, self.fog, self.fog_range, self.shadows);
    }
}

/// A named snapshot of global controls and all custom map overrides.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct LightingPreset {
    pub name: String,
    pub controls: LightingControls,
    pub map_overrides: BTreeMap<String, MapLightingOverrides>,
}

/// Partial changes for one map: its global ambience and optional local areas.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct MapLightingOverrides {
    pub global: BubbleOverrides,
    pub areas: Vec<AreaLightingOverride>,
}

/// One optional field per overridable Bubble field, and their application.
macro_rules! bubble_overrides {
    ($($field:ident: $ty:ty),* $(,)?) => {
        /// Optional fields let untouched properties continue to come from the
        /// original ambience data. Colour arrays use the source's RGBA convention.
        #[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
        #[serde(default)]
        pub struct BubbleOverrides {
            $(#[serde(skip_serializing_if = "Option::is_none")] pub $field: Option<$ty>,)*
        }

        fn apply_bubble_overrides(target: &mut Bubble, values: &BubbleOverrides) {
            $(if let Some(v) = values.$field { target.$field = v; })*
        }
    };
}

bubble_overrides! {
    light_color: [f32; 4],
    light_opposite_color: [f32; 4],
    light_intensity: f32,
    light_shadow_intensity: f32,
    back_light_factor: f32,
    light_ambient: [f32; 4],
    saturation: f32,
    balance: [f32; 4],
    bloom_scale: f32,
    bloom_color: [f32; 4],
    bright_pass_threshold: f32,
    fog_color: [f32; 4],
    fog_intensity: f32,
    fog_near_dist: f32,
    fog_far_dist: f32,
    height_fog_color: [f32; 4],
    fog_near_height: f32,
    fog_far_height: f32,
    rim_light_color: [f32; 4],
    rim_light_intensity: f32,
    local_light_color: [f32; 4],
    local_light_intensity: f32,
    // The original ambience format has no player-light radius field.
    local_light_range: f32,
    vignette_intensity: f32,
    vignette_size: f32,
    vignette_color: [f32; 4],
    fade_time: f32,
    dir: Option<[f32; 3]>,
}

/// Identifies a source ambience bubble as well as its array index. The extra
/// identity fields help detect mismatches if a different cache changes bubble
/// ordering. A missing value leaves that original property unchanged.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct AreaLightingOverride {
    pub bubble_index: usize,
    pub event: i32,
    pub priority: i32,
    pub shape: String,
    pub transform: Option<[f32; 16]>,
    pub values: BubbleOverrides,
}

/// Resolve original ambience values with map-wide overrides followed by a
/// matching local-area override. The source asset is never modified.
pub fn resolve_bubble(source: &Bubble, map: Option<&MapLightingOverrides>, area: Option<(usize, &LocalBubble)>) -> Bubble {
    let mut resolved = source.clone();
    let Some(map) = map else { return resolved };
    apply_bubble_overrides(&mut resolved, &map.global);
    if let Some((index, source_area)) = area {
        if let Some(area_override) = map.areas.iter().find(|candidate| candidate.matches_source(index, source_area)) {
            apply_bubble_overrides(&mut resolved, &area_override.values);
        }
    }
    resolved
}

impl AreaLightingOverride {
    /// Don't apply an area override to a different bubble merely because it
    /// occupies the same index in another cache.
    fn matches_source(&self, index: usize, source: &LocalBubble) -> bool {
        self.bubble_index == index
            && self.event == source.event
            && self.priority == source.priority
            && self.shape == source.shape
            && self.transform == source.m
    }
}

impl LightingMenuStore {
    /// Load local presets. A bad or unsupported file is preserved on disk;
    /// the editor starts with defaults and reports the problem in the log.
    pub fn load() -> Self {
        let raw = match read_store_text() {
            Ok(Some(raw)) => raw,
            Ok(None) => return Self::default(),
            Err(error) => {
                warn!("lighting presets: could not read store: {error}");
                return Self::default();
            }
        };
        match serde_json::from_str::<Self>(&raw) {
            Ok(mut store) if store.schema_version <= SCHEMA_VERSION => {
                // Older files can gain newly added defaulted fields.
                store.schema_version = SCHEMA_VERSION;
                store
            }
            Ok(store) => {
                warn!("lighting presets: store version {} is newer than supported version {SCHEMA_VERSION}; leaving the file untouched", store.schema_version);
                Self::default()
            }
            Err(error) => {
                warn!("lighting presets: invalid JSON; leaving the file untouched: {error}");
                Self::default()
            }
        }
    }

    /// Persist the working overrides and all named profiles.
    pub fn save(&mut self) -> Result<(), String> {
        self.schema_version = SCHEMA_VERSION;
        let raw = serde_json::to_string_pretty(self).map_err(|error| format!("could not encode lighting presets: {error}"))?;
        #[cfg(not(target_arch = "wasm32"))]
        {
            std::fs::write(NATIVE_STORE_FILE, raw).map_err(|error| format!("could not write {NATIVE_STORE_FILE}: {error}"))
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = NATIVE_STORE_FILE;
            let window = web_sys::window().ok_or_else(|| "browser window is unavailable".to_string())?;
            let storage = window.local_storage()
                .map_err(|error| format!("could not access local storage: {error:?}"))?
                .ok_or_else(|| "browser local storage is unavailable".to_string())?;
            storage.set_item(WEB_STORAGE_KEY, &raw).map_err(|error| format!("could not save lighting presets: {error:?}"))
        }
    }

    /// Create a snapshot of the current controls and working map overrides.
    pub fn save_as(&mut self, name: &str, controls: &LightingControls) -> Result<String, String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("preset name cannot be empty".to_string());
        }
        while self.presets.contains_key(&format!("preset-{:04}", self.next_preset_id)) {
            self.next_preset_id = self.next_preset_id.checked_add(1).ok_or_else(|| "no preset IDs remain".to_string())?;
        }
        let id = format!("preset-{:04}", self.next_preset_id);
        self.next_preset_id = self.next_preset_id.saturating_add(1);
        self.presets.insert(id.clone(), LightingPreset {
            name: name.to_string(),
            controls: controls.clone(),
            map_overrides: self.working_map_overrides.clone(),
        });
        self.active_preset_id = Some(id.clone());
        Ok(id)
    }

    /// Select a saved profile and replace the working map overrides.
    /// The caller applies the returned global controls to the live renderer.
    pub fn select_preset(&mut self, id: &str) -> Option<LightingPreset> {
        let preset = self.presets.get(id)?.clone();
        self.working_map_overrides = preset.map_overrides.clone();
        self.active_preset_id = Some(id.to_string());
        Some(preset)
    }

    pub fn overwrite_active(&mut self, controls: &LightingControls) -> Result<(), String> {
        let id = self.active_preset_id.clone().ok_or_else(|| "no active preset is selected".to_string())?;
        let map_overrides = self.working_map_overrides.clone();
        let preset = self.presets.get_mut(&id).ok_or_else(|| "the active preset no longer exists".to_string())?;
        preset.controls = controls.clone();
        preset.map_overrides = map_overrides;
        Ok(())
    }

    pub fn rename_preset(&mut self, id: &str, name: &str) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("preset name cannot be empty".to_string());
        }
        let preset = self.presets.get_mut(id).ok_or_else(|| "preset not found".to_string())?;
        preset.name = name.to_string();
        Ok(())
    }

    /// Delete a saved snapshot; working settings are deliberately preserved.
    pub fn delete_preset(&mut self, id: &str) -> bool {
        let removed = self.presets.remove(id).is_some();
        if removed && self.active_preset_id.as_deref() == Some(id) {
            self.active_preset_id = None;
        }
        removed
    }

    pub fn active_preset_is_modified(&self, controls: &LightingControls) -> bool {
        let Some(id) = self.active_preset_id.as_deref() else { return false };
        let Some(preset) = self.presets.get(id) else { return true };
        preset.controls != *controls || preset.map_overrides != self.working_map_overrides
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn read_store_text() -> Result<Option<String>, String> {
    match std::fs::read_to_string(NATIVE_STORE_FILE) {
        Ok(raw) => Ok(Some(raw)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("could not read {NATIVE_STORE_FILE}: {error}")),
    }
}

#[cfg(target_arch = "wasm32")]
fn read_store_text() -> Result<Option<String>, String> {
    let Some(window) = web_sys::window() else { return Ok(None) };
    let storage = window.local_storage().map_err(|error| format!("could not access local storage: {error:?}"))?;
    let Some(storage) = storage else { return Ok(None) };
    storage.get_item(WEB_STORAGE_KEY).map_err(|error| format!("could not read lighting presets: {error:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preset_store_round_trips_partial_map_overrides() {
        let mut store = LightingMenuStore::default();
        store.working_map_overrides.insert("a0200_kingscity".to_string(), MapLightingOverrides {
            global: BubbleOverrides { fog_far_dist: Some(72.0), local_light_range: Some(11.5), ..default() },
            areas: vec![AreaLightingOverride {
                bubble_index: 2,
                event: -1,
                priority: 100,
                shape: "Box".to_string(),
                transform: None,
                values: BubbleOverrides { vignette_intensity: Some(0.03), rim_light_intensity: Some(0.8), ..default() },
            }],
        });
        let controls = LightingControls { fog: 0.8, ..default() };
        let id = store.save_as("Kingshill test", &controls).unwrap();

        let encoded = serde_json::to_string_pretty(&store).unwrap();
        let restored: LightingMenuStore = serde_json::from_str(&encoded).unwrap();
        assert_eq!(restored, store);
        let preset = restored.presets.get(&id).unwrap();
        assert_eq!(preset.controls.fog, 0.8);
        assert_eq!(preset.map_overrides["a0200_kingscity"].global.fog_far_dist, Some(72.0));
        assert_eq!(preset.map_overrides["a0200_kingscity"].areas[0].values.vignette_intensity, Some(0.03));
        assert_eq!(preset.map_overrides["a0200_kingscity"].areas[0].values.rim_light_intensity, Some(0.8));
    }

    #[test]
    fn omitted_override_fields_stay_unset() {
        let override_values: BubbleOverrides = serde_json::from_str("{}").unwrap();
        assert_eq!(override_values.fog_far_dist, None);
        assert_eq!(override_values.light_color, None);
        assert_eq!(override_values.local_light_range, None);
    }

    #[test]
    fn presets_from_the_2018_studio_still_load() {
        // PR #5's controls had exposure, bloom, saturation, vignette and
        // tonemapping; they are ignored, the rest keeps its values.
        let raw = r#"{"name":"Old","controls":{"sun":1.2,"exposure":0.5,"bloom":2.0,"tonemapping":true,"shadows":false},"map_overrides":{}}"#;
        let preset: LightingPreset = serde_json::from_str(raw).unwrap();
        assert_eq!(preset.controls.sun, 1.2);
        assert!(!preset.controls.shadows);
        assert_eq!(preset.controls.fog_range, Lighting::default().fog_range);
    }

    #[test]
    fn controls_round_trip_the_global_multipliers() {
        let mut lighting = Lighting::default();
        let controls = LightingControls { sun: 1.3, lights: 1.0, shadows: false, ..default() };
        controls.apply(&mut lighting);
        assert_eq!(LightingControls::from(&lighting), controls);
    }

    #[test]
    fn map_and_area_overrides_resolve_in_order_without_mutating_assets() {
        let global_source = Bubble { fog_far_dist: 50.0, ..default() };
        let original_global = global_source.clone();
        let transform = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 4.0, 5.0, 6.0, 1.0];
        let source_area = LocalBubble {
            look: Bubble { fog_far_dist: 60.0, ..default() },
            m: Some(transform),
            priority: 100,
            event: -1,
            shape: "Box".to_string(),
            enabled: true,
        };
        let map = MapLightingOverrides {
            global: BubbleOverrides {
                fog_far_dist: Some(90.0),
                local_light_range: Some(12.0),
                light_color: Some([0.2, 0.3, 0.4, 1.0]),
                ..default()
            },
            areas: vec![AreaLightingOverride {
                bubble_index: 0,
                event: -1,
                priority: 100,
                shape: "Box".to_string(),
                transform: Some(transform),
                values: BubbleOverrides { fog_far_dist: Some(44.0), vignette_intensity: Some(0.03), ..default() },
            }],
        };

        let global = resolve_bubble(&global_source, Some(&map), None);
        assert_eq!(global.fog_far_dist, 90.0);
        assert_eq!(global.local_light_range, 12.0);
        assert_eq!(global.light_color, [0.2, 0.3, 0.4, 1.0]);

        let resolved_area = resolve_bubble(&source_area.look, Some(&map), Some((0, &source_area)));
        assert_eq!(resolved_area.fog_far_dist, 44.0);
        assert_eq!(resolved_area.vignette_intensity, 0.03);
        assert_eq!(resolved_area.local_light_range, 12.0);

        // An override for another bubble at the same index is not applied.
        let moved = LocalBubble { m: None, ..source_area.clone() };
        assert_eq!(resolve_bubble(&moved.look, Some(&map), Some((0, &moved))).fog_far_dist, 90.0);

        // Source ambience stays unchanged.
        assert_eq!(global_source, original_global);
        assert_eq!(source_area.look.fog_far_dist, 60.0);
    }

    #[test]
    fn selecting_a_preset_restores_its_working_overrides() {
        let mut store = LightingMenuStore::default();
        store.working_map_overrides.insert("a0200_kingscity".to_string(), MapLightingOverrides {
            global: BubbleOverrides { fog_near_dist: Some(12.0), ..default() },
            ..default()
        });
        let id = store.save_as("Test", &LightingControls::default()).unwrap();
        store.working_map_overrides.clear();
        let selected = store.select_preset(&id).unwrap();
        assert_eq!(selected.map_overrides["a0200_kingscity"].global.fog_near_dist, Some(12.0));
        assert_eq!(store.working_map_overrides["a0200_kingscity"].global.fog_near_dist, Some(12.0));
        assert!(!store.active_preset_is_modified(&selected.controls));
    }
}
