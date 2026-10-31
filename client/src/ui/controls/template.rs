//! Filling the authored windows from data: every list the 2018 client drew is a
//! template row (its id has a `Template` part) that the client cloned per entry.
//! These helpers clone a template's subtree under a new name, move it, set its
//! texts, pick its "selected" artwork and lay item icons over it; the windows'
//! `prepare` functions (party, quests, systems) build on them.
use crate::ui::{Art, Layout, Widget};

/// The key of a text the client wrote (crate::ui::spawn_windows leaves it as it
/// is: neither localised nor bound to a value).
pub(in crate::ui) const RUNTIME_KEY: &str = "gui.runtime";
const GREY: [f32; 4] = [0.55, 0.55, 0.55, 1.0];

pub(in crate::ui) fn leaf(id: &str) -> &str { id.rsplit('/').next().unwrap_or("") }

pub(in crate::ui) fn set_text(w: &mut Widget, text: &str) {
    if let Some(label) = &mut w.text { label.text = text.into(); label.key = RUNTIME_KEY.into(); }
}

pub(in crate::ui) fn grey(w: &mut Widget) {
    if let Some(t) = &mut w.text { t.color = GREY; }
}

/// Crop the artwork from the right to `fraction` of its width, its UVs with it
/// (a progress bar's fill): each vertex right of the cut moves to it, its u
/// interpolated from its row's leftmost vertex.
pub(in crate::ui) fn clip_x(w: &mut Widget, fraction: f32) {
    let fraction = fraction.clamp(0.0, 1.0);
    let [left, _, right, _] = w.rect;
    let cut = left + (right - left) * fraction;
    w.rect[2] = cut.max(left);
    let Some(art) = &mut w.art else { return };
    let n = art.pos.len() / 2;
    let rows: Vec<(f32, f32, f32)> = (0..n).map(|i| (art.pos[2 * i], art.pos[2 * i + 1], art.uv.get(2 * i).copied().unwrap_or(0.0))).collect();
    for i in 0..n {
        let (x, y, u) = rows[i];
        if x <= cut { continue; }
        // The row's left edge: the leftmost vertex at this height.
        let (lx, _, lu) = rows.iter().filter(|(_, ry, _)| (ry - y).abs() < 0.5).fold((x, y, u), |a, b| if b.0 < a.0 { *b } else { a });
        let t = if (x - lx).abs() < 1e-4 { 1.0 } else { (cut - lx) / (x - lx) };
        art.pos[2 * i] = cut.max(lx);
        if let Some(uv) = art.uv.get_mut(2 * i) { *uv = lu + (u - lu) * t.clamp(0.0, 1.0); }
    }
}

/// Crop the artwork from the top, keeping its lower `fraction` (a rising
/// vertical bar's fill), its V coordinates with it.
pub(in crate::ui) fn clip_y(w: &mut Widget, fraction: f32) {
    let fraction = fraction.clamp(0.0, 1.0);
    let [_, top, _, bottom] = w.rect;
    let cut = bottom - (bottom - top) * fraction;
    w.rect[1] = cut.min(bottom);
    let Some(art) = &mut w.art else { return };
    let n = art.pos.len() / 2;
    let points: Vec<(f32, f32, f32)> = (0..n).map(|i| (art.pos[2 * i], art.pos[2 * i + 1], art.uv.get(2 * i + 1).copied().unwrap_or(0.0))).collect();
    for i in 0..n {
        let (x, y, v) = points[i];
        if y >= cut { continue; }
        // The column's bottom: the lowest vertex at this x.
        let (_, by, bv) = points.iter().filter(|(cx, _, _)| (cx - x).abs() < 0.5).fold((x, y, v), |a, b| if b.1 > a.1 { *b } else { a });
        let t = if (by - y).abs() < 1e-4 { 1.0 } else { (by - cut) / (by - y) };
        art.pos[2 * i + 1] = cut.min(by);
        if let Some(uv) = art.uv.get_mut(2 * i + 1) { *uv = bv + (v - bv) * t.clamp(0.0, 1.0); }
    }
}

/// Fade the artwork (an unowned sticker, a locked socket).
pub(in crate::ui) fn dim(w: &mut Widget, alpha: f32) {
    if let Some(a) = &mut w.art { for c in a.color.chunks_exact_mut(4) { c[3] *= alpha; } }
    if let Some(t) = &mut w.text { t.color[3] *= alpha; }
}

pub(in crate::ui) fn map_y(w: &mut Widget, y: impl Fn(f32) -> f32) {
    w.rect = [w.rect[0], y(w.rect[1]), w.rect[2], y(w.rect[3])];
    if let Some(a) = &mut w.art { for p in a.pos.chunks_exact_mut(2) { p[1] = y(p[1]); } }
}

pub(in crate::ui) fn map_x(w: &mut Widget, x: impl Fn(f32) -> f32) {
    w.rect = [x(w.rect[0]), w.rect[1], x(w.rect[2]), w.rect[3]];
    if let Some(a) = &mut w.art { for p in a.pos.chunks_exact_mut(2) { p[0] = x(p[0]); } }
}

pub(in crate::ui) fn shift(w: &mut Widget, dx: f32, dy: f32) {
    if dx != 0.0 { map_x(w, |x| x + dx); }
    if dy != 0.0 { map_y(w, |y| y + dy); }
}

/// The template's subtree, its first `template` part renamed, moved by (dx, dy).
pub(in crate::ui) fn clone_at<'a>(source: &'a Layout, template: &str, replacement: &str, dx: f32, dy: f32) -> impl Iterator<Item = Widget> + 'a {
    let prefix = format!("{template}/");
    let (root, template, replacement) = (template.to_owned(), template.to_owned(), replacement.to_owned());
    source.widgets.iter().filter(move |w| w.id == root || w.id.starts_with(&prefix)).map(move |widget| {
        let mut w = widget.clone();
        w.id = w.id.replacen(&template, &replacement, 1);
        w.button = w.button.map(|b| b.replacen(&template, &replacement, 1));
        shift(&mut w, dx, dy);
        w
    })
}

/// The template's subtree, renamed, shifted down by `dy`.
pub(in crate::ui) fn clone_rows<'a>(source: &'a Layout, template: &str, replacement: &str, dy: f32) -> impl Iterator<Item = Widget> + 'a {
    clone_at(source, template, replacement, 0.0, dy)
}

/// Keep the source's pressed artwork for the open row (as the merchant's tabs).
pub(in crate::ui) fn selected(w: &mut Widget) {
    if let Some(state) = &mut w.state {
        match state.as_str() {
            "normal" | "mouseover" => w.hidden = true,
            "pressed" => *state = "normal".into(),
            "mouseoverpressed" => *state = "mouseover".into(),
            _ => {}
        }
    }
}

pub(in crate::ui) fn wrap(text: &str, width: usize, most: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() && line.chars().count() + word.chars().count() + 1 > width {
            lines.push(std::mem::take(&mut line));
            if lines.len() == most { return lines; }
        }
        if !line.is_empty() { line.push(' '); }
        line.push_str(word);
    }
    if !line.is_empty() { lines.push(line); }
    lines
}

pub(in crate::ui) fn image_art(rect: [f32; 4], texture: String) -> Art {
    let [l, t, r, b] = rect;
    Art { tex: Some(texture), pos: vec![l, t, r, t, r, b, l, b], uv: vec![0., 0., 1., 0., 1., 1., 0., 1.], color: vec![1.; 16], idx: vec![0, 1, 2, 0, 2, 3], saturation: 1.0 }
}

/// An item's icon as the inventory draws it: the rarity background, the image,
/// the frame (crate::ui::controls::items::Catalog::icon_parts).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::ui) struct Icon {
    pub background: String,
    pub image: String,
    pub frame: String,
    /// Over the frame: a time-limited item's hourglass (items::Catalog::icon_info).
    pub info: String,
}

/// The icon's layers in place of the template's Icon widget (`w`), named
/// `<w.id>/<Background|Image|Frame>`; the widget itself keeps its text only.
pub(in crate::ui) fn icon_layers(w: &mut Widget, icon: &Icon) -> Vec<Widget> {
    let mut layers = Vec::new();
    for (suffix, texture) in [("Background", &icon.background), ("Image", &icon.image), ("Frame", &icon.frame), ("Info", &icon.info)] {
        if texture.is_empty() { continue; }
        let mut layer = w.clone();
        layer.id = format!("{}/{suffix}", w.id);
        layer.text = None;
        layer.hidden = false;
        layer.button = None;
        layer.state = None;
        layer.art = Some(image_art(layer.rect, texture.clone()));
        layers.push(layer);
    }
    w.art = None;
    layers
}

/// The source without the widgets `drop` names (the templates, sample rows,
/// parts another situation shows).
pub(in crate::ui) fn without(source: &Layout, drop: impl Fn(&str) -> bool) -> Layout {
    Layout { window: source.window.clone(), widgets: source.widgets.iter().filter(|w| !drop(&w.id)).cloned().collect() }
}

/// Whether a path has a part starting with `Template`.
pub(in crate::ui) fn is_template(id: &str) -> bool {
    id.split('/').any(|p| p.starts_with("Template"))
}

pub(in crate::ui) fn has_part(id: &str, part: &str) -> bool {
    id.split('/').any(|p| p == part)
}

/// Rename every path part `from` of the widget (and of the button its state
/// belongs to) `to`: the clones' inner templates (TemplateSlot, TemplateIcon).
pub(in crate::ui) fn rename(w: &mut Widget, from: &str, to: &str) {
    let renamed = |id: &str| id.split('/').map(|p| if p == from { to } else { p }).collect::<Vec<_>>().join("/");
    w.id = renamed(&w.id);
    w.button = w.button.as_deref().map(renamed);
}

/// A button's artwork states in the clone: the root becomes a Button with the
/// event, its state children keep pointing at it (renamed by clone_at).
pub(in crate::ui) fn make_button(w: &mut Widget, event: &str) {
    w.kind = "Button".into();
    w.event = event.into();
    w.hidden = false;
    w.click_through = false;
}

#[cfg(test)]
pub(in crate::ui) mod fixture {
    use super::*;

    /// A layout of (id, kind, has text) widgets: a button's state children carry
    /// their state and their button, as exported.
    pub(in crate::ui) fn layout(window: &str, ids: &[(&str, &str, bool)]) -> Layout {
        let (uv, color) = (vec![0.0f32; 8], vec![1.0f32; 16]);
        let widget = |(id, kind, text): &(&str, &str, bool)| {
            let leaf = leaf(id);
            let state = ["normal", "pressed", "mouseover", "disabled"].contains(&leaf).then(|| leaf.to_owned());
            let button = state.as_ref().map(|_| id.rsplit_once('/').map(|(p, _)| p.to_owned()).unwrap_or_default());
            serde_json::json!({
                "id": id, "type": kind, "k": 0.5, "rect": [10.0, 192.0, 100.0, 238.0], "hidden": false, "state": state, "button": button,
                "art": {"tex": null, "pos": [10.0, 192.0, 100.0, 192.0, 100.0, 238.0, 10.0, 238.0], "uv": uv, "color": color, "idx": [0, 1, 2, 0, 2, 3]},
                "text": if *text { serde_json::json!({"text": "Text", "key": "Text", "size": 10.0, "color": [1.0, 1.0, 1.0, 1.0], "align": [0, 1]}) } else { serde_json::Value::Null },
            })
        };
        serde_json::from_value(serde_json::json!({ "window": window, "widgets": ids.iter().map(widget).collect::<Vec<_>>() })).unwrap()
    }

    pub(in crate::ui) fn text_of(layout: &Layout, id: &str) -> String {
        layout.widgets.iter().find(|w| w.id == id).and_then(|w| w.text.as_ref()).map(|t| t.text.clone()).unwrap_or_default()
    }

    pub(in crate::ui) fn find<'a>(layout: &'a Layout, id: &str) -> &'a Widget {
        layout.widgets.iter().find(|w| w.id == id).unwrap_or_else(|| panic!("no widget {id}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clones_rename_move_and_keep_button_states() {
        let source = fixture::layout("w", &[("A/TemplateRow", "Button", false), ("A/TemplateRow/normal", "Label", false), ("A/TemplateRow/pressed", "Label", false), ("A/TemplateRow/Name", "TextLabel", true), ("A/Other", "Label", false)]);
        let rows: Vec<_> = clone_at(&source, "A/TemplateRow", "A/Row3", 5.0, 46.0).collect();
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0].id, "A/Row3");
        assert_eq!((rows[0].rect, rows[0].art.as_ref().unwrap().pos[0..2].to_vec()), ([15.0, 238.0, 105.0, 284.0], vec![15.0, 238.0]));
        assert_eq!(rows[1].button.as_deref(), Some("A/Row3"));
        let mut open = rows[2].clone();
        selected(&mut open);
        assert_eq!(open.state.as_deref(), Some("normal"));
        let mut closed = rows[1].clone();
        selected(&mut closed);
        assert!(closed.hidden);
        assert_eq!(wrap("aa bb cc dd", 5, 9), ["aa bb", "cc dd"]);
        assert_eq!(wrap("aa bb cc dd", 5, 1), ["aa bb"]);
        assert!(is_template("A/TemplateRow/Name") && !is_template("A/Row3/Name"));
    }

    #[test]
    fn icon_layers_replace_the_artwork_in_order() {
        let source = fixture::layout("w", &[("A/Slot/Icon", "Icon", true)]);
        let mut w = source.widgets[0].clone();
        let layers = icon_layers(&mut w, &Icon { background: "bg.png".into(), image: "img.png".into(), frame: String::new(), ..Default::default() });
        assert_eq!(layers.iter().map(|l| l.id.as_str()).collect::<Vec<_>>(), ["A/Slot/Icon/Background", "A/Slot/Icon/Image"]);
        assert_eq!(layers[1].art.as_ref().unwrap().tex.as_deref(), Some("img.png"));
        assert!(w.art.is_none() && w.text.is_some());
        let mut faded = layers[0].clone();
        dim(&mut faded, 0.5);
        assert_eq!(faded.art.as_ref().unwrap().color[3], 0.5);
    }
}
