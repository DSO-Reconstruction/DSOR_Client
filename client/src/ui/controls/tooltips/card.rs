//! Flow the XML tooltip templates around measured text, then place the whole
//! card beside its owner. Coordinates are reference pixels, as for the HUD.
use super::*;

pub(in crate::ui::controls) const WHITE: Color = Color::srgb(0.92, 0.92, 0.92);
pub(in crate::ui::controls) const MUTED: Color = Color::srgb(0.70, 0.70, 0.70);
pub(in crate::ui::controls) const BLUE: Color = Color::srgb(0.48, 0.69, 1.0);
pub(in crate::ui::controls) const GOLD: Color = Color::srgb(0.85, 0.67, 0.0);
pub(in crate::ui::controls) const GREEN: Color = Color::srgb(0.1, 0.85, 0.1);

#[derive(Clone, Debug, PartialEq)]
enum Part {
    Text { value: String, at: Vec2, size: f32, color: Color, italic: bool, bold: bool },
    Icon { path: String, at: Vec2, size: f32 },
    Art { template: String, rect: [f32; 4] },
}

#[derive(Clone, Debug, PartialEq)]
pub(in crate::ui) struct Card {
    pub width: f32,
    pub y: f32,
    pub background: Color,
    size: f32,
    small: f32,
    title: f32,
    parts: Vec<Part>,
}

impl Card {
    #[cfg(test)]
    pub fn text_content(&self) -> String {
        self.parts.iter().filter_map(|p| match p { Part::Text {value,..} => Some(value.as_str()), _=>None })
            .collect::<Vec<_>>().join(" ")
    }

    pub fn new(width: f32, layout: Option<&Layout>) -> Self {
        let font = |name: &str, fallback| layout.and_then(|l| l.widgets.iter().find(|w| w.id.ends_with(name)))
            .and_then(|w| w.text.as_ref()).map_or(fallback, |t| t.size * FONT_SCALE);
        Self { width, y: 16.0, background: Color::srgba(0.015, 0.015, 0.01, 0.94), size: font("/TextLabelDefaultTemplate", 12.0),
            small: font("/TextLabelSmallTemplate", 10.8), title: font("/TextLabelHeadlineTemplate", 14.4), parts: vec![] }
    }

    pub fn height(&self) -> f32 { self.y + 16.0 }

    /// The body text size (reference pixels), for a card sized to its text.
    pub fn text_size(&self) -> f32 { self.size }

    /// Every text size times `k` (the one-line tooltips, controls::tooltips).
    pub fn shrink(&mut self, k: f32) {
        self.size *= k;
        self.small *= k;
        self.title *= k;
    }

    pub fn title(&mut self, value: &str, icon: Option<&str>, color: Color) {
        let start = self.y;
        if let Some(path) = icon {
            self.parts.push(Part::Icon { path: path.into(), at: Vec2::new(16.0, start), size: 32.0 });
            self.y += 7.0;
            self.paragraph(value, color, self.title, 58.0, false, true);
            self.y = self.y.max(start + 36.0);
        } else {
            self.paragraph(value, color, self.title, 16.0, true, true);
            self.y += 3.0;
        }
    }

    pub fn text(&mut self, value: &str, color: Color) {
        self.paragraph(value, color, self.size, 16.0, false, false);
    }

    pub fn hint(&mut self, value: &str) {
        self.paragraph(value, MUTED, self.small, 16.0, false, false);
    }

    pub fn centered(&mut self, value: &str, color: Color, small: bool) {
        self.paragraph(value, color, if small { self.small } else { self.size }, 12.0, true, false);
    }

    pub fn gap(&mut self, amount: f32) { self.y += amount; }

    pub fn rule(&mut self) {
        self.parts.push(Part::Art { template: "SeperatorLineTemplate".into(), rect: [7.0, self.y, self.width - 7.0, self.y + 5.0] });
        self.y += 9.0;
    }

    pub fn section(&mut self, name: &str) {
        self.gap(4.0);
        self.rule();
        self.centered(name, MUTED, true);
        self.gap(3.0);
    }

    pub fn icons(&mut self, icons: impl Iterator<Item = String>) {
        let mut x = 16.0;
        let mut any = false;
        for path in icons {
            if x + 21.0 > self.width - 16.0 { x = 16.0; self.y += 25.0; }
            self.parts.push(Part::Icon { path, at: Vec2::new(x, self.y), size: 21.0 });
            x += 23.0;
            any = true;
        }
        if any { self.y += 26.0; }
    }

    pub fn price(&mut self, value: u32) {
        let size = self.small;
        let label = "Selling price: ";
        let mut x = 16.0 + measure(label, size);
        self.parts.push(Part::Text { value: label.into(), at: Vec2::new(16.0, self.y + 3.0), size, color: MUTED, italic: false, bold: false });
        for (name, amount) in [("Gold", value / 10000), ("Silver", value / 100 % 100), ("Copper", value % 100)] {
            if amount == 0 && name != "Copper" { continue; }
            let text = amount.to_string();
            let width = 20.0 + measure(&text, size);
            if x + width > self.width - 16.0 { x = 16.0; self.y += 22.0; }
            self.parts.push(Part::Art { template: format!("ItemValueWeaponEffectsLabelTemplate/CurrencyLabel/{name}Label"), rect: [x, self.y, x + 17.0, self.y + 17.0] });
            self.parts.push(Part::Text { value: text, at: Vec2::new(x + 18.0, self.y + 3.0), size, color: WHITE, italic: false, bold: false });
            x += width + 5.0;
        }
        self.y += 24.0;
    }

    pub fn melting(&mut self, value: u32) {
        let x = 16.0 + measure("Melting value: ", self.small);
        self.parts.push(Part::Text { value: "Melting value: ".into(), at: Vec2::new(16.0,self.y+3.0), size:self.small,color:MUTED,italic:false,bold:false });
        self.parts.push(Part::Art { template:"ItemValueWeaponEffectsLabelTemplate/CurrencyLabel/UCLabel".into(),rect:[x,self.y,x+17.0,self.y+17.0] });
        self.parts.push(Part::Text { value:value.to_string(),at:Vec2::new(x+19.0,self.y+3.0),size:self.small,color:WHITE,italic:false,bold:false });
        self.y += 24.0;
    }

    fn paragraph(&mut self, value: &str, color: Color, size: f32, left: f32, center: bool, bold: bool) {
        let width = (self.width - left - 16.0).max(size);
        let glyphs = rich(value, color);
        // Wrap whole words, splitting an overlong word only when necessary. The
        // same glyph advances determine line heights and the rendered positions.
        let mut line: Vec<(char, Color, bool)> = vec![];
        let mut used = 0.0;
        let mut i = 0;
        while i < glyphs.len() {
            if glyphs[i].0 == '\n' {
                self.line(&line, size, left, width, center, bold);
                line.clear(); used = 0.0; i += 1; continue;
            }
            let start = i;
            if glyphs[i].0.is_whitespace() { i += 1; }
            else { while i < glyphs.len() && !glyphs[i].0.is_whitespace() { i += 1; } }
            let word = &glyphs[start..i];
            let word_width: f32 = word.iter().map(|g| measure(&g.0.to_string(), size)).sum();
            if used > 0.0 && used + word_width > width {
                self.line(&line, size, left, width, center, bold);
                line.clear(); used = 0.0;
            }
            for &g in word {
                if line.is_empty() && g.0.is_whitespace() { continue; }
                let advance = measure(&g.0.to_string(), size);
                if used > 0.0 && used + advance > width {
                    self.line(&line, size, left, width, center, bold);
                    line.clear(); used = 0.0;
                }
                line.push(g); used += advance;
            }
        }
        if !line.is_empty() { self.line(&line, size, left, width, center, bold); }
    }

    fn line(&mut self, line: &[(char, Color, bool)], size: f32, left: f32, width: f32, center: bool, bold: bool) {
        let value: String = line.iter().map(|g| g.0).collect();
        let mut x = left + if center { (width - measure(value.trim_end(), size)).max(0.0) / 2.0 } else { 0.0 };
        let mut i = 0;
        while i < line.len() {
            let start = i;
            let (_, color, italic) = line[i];
            while i < line.len() && line[i].1 == color && line[i].2 == italic { i += 1; }
            let value: String = line[start..i].iter().map(|g| g.0).collect();
            let advance = measure(&value, size);
            self.parts.push(Part::Text { value, at: Vec2::new(x, self.y), size, color, italic, bold });
            x += advance;
        }
        self.y += size * 1.3;
    }

    pub fn draw(&self, commands: &mut Commands, assets: &AssetServer, screen: &Screen,
        meshes: &mut Assets<Mesh>, materials: &mut Assets<UiMaterial>, root: Entity, layout: Option<&Layout>) {
        if let Some(layout) = layout { items::frame(commands, assets, meshes, materials, root, layout, self.width, self.height(), self.background); }
        else { background(commands, root, self.width, self.height(), [4.0; 4], self.background); }
        for part in &self.parts {
            match part {
                Part::Text { value, at, size, color, italic, bold } => {
                    commands.spawn((Text2d::new(value.clone()), TextFont { font: assets.load("fonts/tahoma.ttf").into(),
                        font_size: bevy::text::FontSize::Px(size * screen.scale),
                        style: if *italic { bevy::text::FontStyle::Italic } else { default() },
                        weight: if *bold { bevy::text::FontWeight::BOLD } else { default() }, ..default() }, TextColor(*color),
                        Anchor::TOP_LEFT, Transform::from_xyz(at.x, -at.y, 1.0).with_scale(Vec3::splat(1.0 / screen.scale)),
                        RenderLayers::layer(LAYER), ChildOf(root)));
                }
                Part::Icon { path, at, size } => { commands.spawn((Sprite { image: assets.load(path.clone()), custom_size: Some(Vec2::splat(*size)), ..default() },
                    Anchor::TOP_LEFT, Transform::from_xyz(at.x, -at.y, 1.0), RenderLayers::layer(LAYER), ChildOf(root))); }
                Part::Art { template, rect } => {
                    let Some(widget) = layout.and_then(|l| l.widgets.iter().find(|w| w.id == format!("Canvas/BGLabel/{template}"))) else { continue; };
                    let Some(mut art) = widget.art.clone() else { continue; };
                    for p in art.pos.chunks_exact_mut(2) {
                        p[0] = rect[0] + (p[0] - widget.rect[0]) / (widget.rect[2] - widget.rect[0]) * (rect[2] - rect[0]);
                        p[1] = -(rect[1] + (p[1] - widget.rect[1]) / (widget.rect[3] - widget.rect[1]) * (rect[3] - rect[1]));
                    }
                    let atlas = art.tex.as_ref().map(|p| assets.load(p.clone()));
                    let mut run = Run::default(); run.push(&art, 0.0);
                    commands.spawn((Mesh2d(meshes.add(run.mesh())), MeshMaterial2d(materials.add(material(atlas))),
                        Transform::from_xyz(0.0, 0.0, 0.5), RenderLayers::layer(LAYER), ChildOf(root)));
                }
            }
        }
    }
}

fn measure(value: &str, size: f32) -> f32 {
    value.chars().map(|c|match c {
        '—'|'…'=>size, '–'=>size*0.5,
        _=>crate::network::chat::text_width(&c.to_string(),size),
    }).sum()
}

pub(in crate::ui::controls) fn background(commands: &mut Commands, root: Entity, width: f32, height: f32, inset: [f32; 4], color: Color) {
    let [l, t, r, b] = inset;
    commands.spawn((Sprite::from_color(color, Vec2::new((width-l-r).max(0.0), (height-t-b).max(0.0))),
        Anchor::TOP_LEFT, Transform::from_xyz(l, -t, 0.0), RenderLayers::layer(LAYER), ChildOf(root)));
}

/// The layout owns the anchor, never the cursor. Flip to a free side before
/// clamping; exceptionally tall cards scale as a unit so no rows are discarded.
pub(in crate::ui::controls) fn placement(anchor: [f32; 4], size: Vec2, viewport: Vec2, compact: bool) -> (Vec2, f32) {
    let margin = 8.0;
    let scale = ((viewport.x - 2.0*margin).max(1.0) / size.x)
        .min((viewport.y - 2.0*margin).max(1.0) / size.y).min(1.0);
    let size = size * scale;
    let [l,t,r,b] = anchor;
    let center = (l+r-size.x)*0.5;
    // A one-line tip opens below its control, but above one in the screen's
    // lower half (the bottom bar's gauges), as the original's open upwards.
    // FAILURE (2026-10-24): made smaller, the gauges' tips fitted below the bar
    //   and covered the skill slots.
    let low = (t+b)*0.5 > viewport.y*0.5;
    let candidates = if compact && low {
        [Vec2::new(center,t-size.y-8.0), Vec2::new(center,b+8.0), Vec2::new(r+8.0,t), Vec2::new(l-size.x-8.0,t)]
    } else if compact {
        [Vec2::new(center,b+8.0), Vec2::new(center,t-size.y-8.0), Vec2::new(r+8.0,t), Vec2::new(l-size.x-8.0,t)]
    } else {
        [Vec2::new(r+8.0,t), Vec2::new(l-size.x-8.0,t), Vec2::new(center,t-size.y-8.0), Vec2::new(center,b+8.0)]
    };
    let clamp = |p: Vec2| p.clamp(Vec2::splat(margin), (viewport-size-Vec2::splat(margin)).max(Vec2::splat(margin)));
    let at = candidates.into_iter().map(clamp).find(|p| p.x+size.x <= l || p.x >= r || p.y+size.y <= t || p.y >= b)
        .unwrap_or_else(|| clamp(candidates[0]));
    (at,scale)
}

/// Keep prose containing unresolved formulas. An em dash denotes an unavailable
/// value; deleting the whole line used to remove nearly every damage effect.
fn rich(raw: &str, base: Color) -> Vec<(char, Color, bool)> {
    let normalized = raw.replace("\\\\n", "\n").replace("\\n", "\n");
    let mut chars = normalized.chars().peekable();
    let mut stack = vec![(base, false)];
    let mut result = vec![];
    while let Some(c) = chars.next() {
        if c == '<' {
            let tag: String = chars.by_ref().take_while(|c| *c != '>').collect();
            if tag.starts_with('/') { if stack.len() > 1 { stack.pop(); } }
            else {
                let (color, italic) = *stack.last().unwrap();
                stack.push((match tag.as_str() { "WhiteStart" => WHITE, "GemBlueStart" | "MagicStart" => BLUE,
                    "UniqueStart" | "SkillHighlightStart" => GOLD, "GreenStart" => GREEN, _ => color }, italic || tag == "i"));
            }
        } else if c == '{' {
            for next in chars.by_ref() { if next == '}' { break; } }
            result.push(('—', MUTED, false));
        } else { let (color, italic) = *stack.last().unwrap(); result.push((c,color,italic)); }
    }
    result
}

pub(in crate::ui::controls) fn plain(raw: &str) -> String { rich(raw, WHITE).iter().map(|g| g.0).collect() }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cards_fit_all_edges_and_avoid_the_owner() {
        for viewport in [Vec2::new(1024.0,768.0), Vec2::new(600.0,768.0), Vec2::new(1800.0,768.0)] {
            for x in [0.0, viewport.x/2.0, viewport.x-45.0] {
                for y in [0.0,350.0,730.0] {
                    let anchor = [x,y,x+40.0,y+30.0];
                    for compact in [false,true] {
                        let size = Vec2::new(320.0,350.0);
                        let (p,s) = placement(anchor,size,viewport,compact);
                        assert!(p.min_element() >= 8.0);
                        assert!(p.x+size.x*s <= viewport.x-7.9 && p.y+size.y*s <= viewport.y-7.9);
                        assert!(p.x+size.x*s <= x || p.x >= x+40.0 || p.y+size.y*s <= y || p.y >= y+30.0);
                    }
                }
            }
        }
        let (p,s) = placement([900.0,700.0,950.0,750.0],Vec2::new(400.0,2000.0),Vec2::new(1024.0,768.0),false);
        assert!(s < 1.0 && p.y+2000.0*s <= 760.1);
    }
    #[test]
    fn rich_descriptions_preserve_effects_and_wrap_inside_content() {
        let text = "Fire.\\\\n\\\\n• Deals {0:t:skill_dmg}% as <i>physical</> damage";
        assert_eq!(plain(text), "Fire.\n\n• Deals —% as physical damage");
        assert!(rich(text,WHITE).iter().any(|g| g.0=='p' && g.2));
        let mut card = Card::new(130.0,None);
        card.text(&format!("{} {}",text,"W".repeat(120)),WHITE);
        for part in &card.parts {
            if let Part::Text { value,at,size,.. } = part {
                assert!(at.x+measure(value,*size) <= card.width-15.9);
                assert!(at.y+size < card.height());
            }
        }
    }
}
