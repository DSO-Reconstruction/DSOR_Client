# The interface (2018 windows)

## The 2018 data

- `export_win32/ui/<window>.bxml`: one window per file (145), Nebula binary XML.
  A tree of widgets (`Label`, `TextLabel`, `Button`, `SlotButton`, `Slot`,
  `ProgressBar`...), each with a `rect` in fractions of a 1024 x 768 reference
  screen, its anchor to its parent (`HorizontalParentAlignment`,
  `VerticalParentAlignment`; none = top left), its artwork (`texture` +
  `meshGrpIdx`) and text (`font`, `fontSize`, `fontColor`, `Text`).
- `export_win32/meshes/ui/<window>_s_0.nvx2`: the window's artwork as one mesh;
  a widget's `meshGrpIdx` is a group of it, with its own triangles, atlas UVs and
  vertex colours (quads, nine-slices, the globes' gradient polygons).
- `export_win32/textures/ui/*`: the atlases; `textures/icons/*`: item and skill
  icons (`_Template_Skill.IconBrush` in `db/static.db4`).
- `export_win32/language/<lang>/*.xml`: the `gui.*` texts.

DSO_Godot's `export_ui.py` and `dsoexp/bxml.py` decode all of it; this
repository's `tools/export_ui.py` reuses them.

## The export (`tools/export_ui.py <export_win32> <assets> <DSO_Godot> [window ...]`)

Every window by default (`--hud`: the four the HUD needs) into
`<assets>/interface/<window>.ui.json`, plus `skills.icons.json` (skill id ->
icon), `loca_fr.json` / `loca_en.json`, and the atlases and icons as PNG under
`<assets>/textures/`. 145 windows, ~32 000 widgets, ~435 000 triangles, 33 MB.

Each widget: its id path, its rect in reference pixels, `k`, its artwork
(triangles, UVs, vertex colours, atlas) and its text (French, else English).
`k` is how far the widget follows the screen's extra width: the client scales the
interface by the window height and widens the reference screen; widget sizes are
fixed and only full-screen top-level canvases stretch, so a widget's x is its
1024 x 768 x plus k x (reference width - 1024). The bottom bar (centred) has
k = 0.5, the money (top left) 0, the menu buttons (top right) 1.

What the game shows only in some state is exported `hidden`: button states
(`mouseover`, `pressed`, `disabled`), cooldown covers, glows, the "buy a slot"
offers, the alternative stacks DSO_Godot already spots, the character preview
area (`figur`, a runtime render target), placeholder icons (`icon_dummy`, "?"),
templates outside a window's root (`ROOTS`: the inventory's drag ghosts, the
payment variants of the money display). Template slots the game clones are laid
out (`GRIDS`: the inventory bag, 7 x 3 empty slots).

## The client (`client/src/ui`)

A 2D camera over the 3D one (sharing its target textures: same HDR and MSAA),
orthographic, 768 reference pixels high. Each window is built once into a few
meshes (one per run of widgets sharing an atlas, the draw order kept by depth),
drawn by `UiMaterial` (`ui.wgsl`: atlas x vertex colour in gamma space, cut to a
clip rectangle). A resize only moves the per-k parents. Texts are Text2d.

Drawn:

- always: the bottom bar (`bottombar`: frame, health globe, the class's resource
  globe, XP bar, 7 quick slots + left / right mouse slots with the skills'
  icons, mount slot, home teleport), the money (`currencybar`), the menu buttons
  (`gamebar`);
- menus: inventory (I: equipment slots, bag tabs I-VIII, 7 x 3 bag, page),
  character sheet (C), skills (K). Natively `DSOR_UI_OPEN=inventory,...` opens
  them from the start (screenshots).

Live values: health and resource (`Net::health` / `resource`; the maxima are
the largest values seen), the quick slot bar (`Net::bar`, wire slots: 0 left
button, 1 right button, 2-6 keys 1-5), the character name. Offline (map viewer)
the HUD shows resting values (80 % health, 60 % resource, 35 % XP, the mage's
skills).

## Missing

- A font with accents: bevy's default font is ASCII only ("D?g?ts"). The 2018
  fonts are bitmaps (`ui/*_info.bytes` + textures), not loaded; a TTF in
  `assets/fonts/` would do.
- Values the client does not decode yet: money, Andermant, level, XP, maximum
  health / resource, buffs, the character sheet's figures, the inventory's items
  (shown as 0, -, empty).
- Interaction: no clicks (buttons, drag and drop, tooltips), no hover states;
  menus open by key only.
- The other 140 windows are exported but not shown (quest log, map, shop,
  chat...); `MENU_WINDOWS` in `client/src/ui/mod.rs` adds one with a key.
- The potion slots drawn inside the globes' edges are as the data places them;
  UNVERIFIED against the 2018 client.
