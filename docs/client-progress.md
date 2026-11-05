# Client feature gaps and implementation progress

Updated 2026-10-14. The baseline compares the local release261 Python server in
`.local/dso_server` with this Rust client. Opening a window does not mean its
gameplay service is connected.

## Current work

- [x] Audit server handlers against client networking and UI.
- [x] Verify the existing protocol tests (8 passed).
- [x] Retain authoritative inventory contents and apply incremental updates.
- [x] Populate equipment, bag items, stack counts and ingredient collection slots.
- [x] Render item tooltips using the original XML templates and item data.
- [x] Implement dragging between slots, dropping compatible stacks to merge,
      and Ctrl+left-click stack splitting with server-assigned item IDs.
- [x] Apply server position corrections, including `ForceTransform`.
- [x] Test protocol compatibility, inventory actions, UI behavior and corrections.
- [x] Push inventory work to `origin/release261` (`1fa3082`).
- [x] Resolve equipped models from the server's item, equipment-skin and appearance tables.
- [x] Apply transmog, dyes and equip/unequip changes to local and remote players.
- [x] Finish appearance regression tests and native visual inspection.

World drops, ground-item rendering and pickup are explicitly deferred.

## Feature backlog

| Feature | Client status at audit | Remaining work |
| --- | --- | --- |
| Inventory | Equipment appearance and vitals read from packets; item contents discarded | Current work above; later purchases, bank access and additional item actions |
| Equipped appearance | Direct legacy Skin lookup only; dyes/transmog ignored; remote updates could precede spawn | Implemented and validated below; remaining missing meshes require asset conversion |
| World drops | No ground-item controller | Render, update, remove and pick up drops; deferred |
| Death and revival | Monster deaths handled | Player death state, death dialog, respawn choices and revive updates |
| Position corrections | Local Move ignored; ForceTransform not decoded | Current work above |
| NPC interaction | NPCs render | Dialogue, service selection and encounter requests |
| Quests and props | Quest window navigation; DB4 prop rendering and server visibility/state updates | Log, objectives, accept/turn-in, markers, prop interactions and triggers |
| Vendors and crafting | Panels only | Offers, buy/sell/buyback, repair, salvage, crafting, sockets and customization |
| Talents | Skill book and hotbar assignment work | Allocation, resets, presets, wisdom/group/honor progression |
| Character statistics | Legacy base-stat calculator; stats asset absent locally | Release261 data, equipment and talent contributions, reliable maxima |
| Chat and social | Social tabs only | Chat connection, player search, group/guild rosters, invitations and group finder |
| Travel | Ordinary map exits work; travel stones render | Travel-map requests, unlocked destinations, travel items and home teleport |
| Events and rewards | Panels only | Event data, participation, notifications, daily rewards/challenges, achievements and wanted missions |
| PvP and rankings | Panels only | Flag, queues, acceptance, battleground state/results, player targeting and ranking rows |
| Mounts and cosmetics | No collection controller | Ownership, activation, favorites and pet upgrades |
| Character management | Login auto-selects a character | Roster screen, deletion, full creation controls and in-game logout |

## Server limits

Friend-list modifications are also missing on the server (`services/social.py`).
Ranking supports lifetime honor/PvP results; historical boards return empty
results (`services/ranking.py`). The prop-interaction path still leaves chest
loot unimplemented (`services/map_events.py`). These are not client-only gaps.

## Evidence and validation

- Server command routing: `.local/dso_server/services/dispatch.py`.
- Client packet application: `client/src/network/net.rs::on_command`.
- Client panel actions: `client/src/ui/controls.rs` and `controls/panels.rs`.
- Existing interface implementation: [UI documentation](ui.md).
- Baseline: `cargo test --offline --locked --release -p dsor-proto` passed all
  8 tests. The initial audit was static; the account endpoint was unreachable.

## Implementation results

`client/src/gameplay/inventory.rs` retains item records across complete allocation updates,
removals and NEW-to-OLD replacement maps. The client also decodes the server's
compact item-update layout correctly. `controls/items.rs` supplies icons, counts,
collection placeholders, tooltips and inventory interactions. The exported
catalog contains 15,334 item definitions and 393 live ingredient collection rows.
The original tooltip border and split dialog come from the XML export.

The client sends ordinary move/swap requests, storage-specific merge requests,
and split requests containing the amount **remaining** in the source stack.
Amounts and IDs remain server-authoritative. The ordinary bag exposes all
unlocked slots through eight 28-slot tabs. The premium bag exposes 28 slots
when membership is active. Loose inventory has its own paged, read-only view
and automatic server recovery.

The inspected server supported splitting but lacked manual merge operations
0x0D-0x10. Its local source now validates compatible stacks, limits quantities,
preserves overflow and sale values, rekeys changed items, sends both inventory
updates and persists the account. The portable change, including tests, is
[`tools/server_inventory_merge.patch`](../tools/server_inventory_merge.patch).
For a fresh copy of this server, run from its root:

```powershell
git apply --ignore-space-change D:/dso/tools/server_inventory_merge.patch
```

The patch is already applied to `.local/dso_server`; restart an existing server
process to load it. No live account inventory was changed during this work.

`ForceTransform` (113) now sets the authoritative absolute position and heading,
clears stale movement/cast holds, and updates local or remote players. Local
Move echoes are compared to the original sent record by tick: ordinary delayed
echoes are ignored, while changed coordinates are applied as corrections.
Map/character changes reset reconciliation history.

## Completed validation

The original 3-row/11-page interpretation was corrected after the user supplied
the bag rules: 4 x 7 slots in bags I-IX, membership-gated bag IX, read-only loose
items in bag X, and paging controls only in bag X. Ingredient tier selectors
now render above their background. `MembershipUpdate` (36) is decoded and
disables premium access when the server expires it.

The server already implemented expiry, persistence and automatic restoration,
but did not transmit loose inventory items. Its serializer now projects those
records into the overflow allocation for display while keeping them outside
the server's usable item table. The additional portable patch is
[`tools/server_inventory_overflow.patch`](../tools/server_inventory_overflow.patch),
already applied locally. Apply it from a fresh server root with
`git apply --ignore-space-change D:/dso/tools/server_inventory_overflow.patch`.
The membership/overflow regression suite passes 24 tests, including idle expiry,
reconnect persistence, renewal, and restoring a freed slot in the same request.

- `cargo test --offline --locked --release -p dsor-client -p dsor-proto -- --include-ignored`:
  **39 client tests and 10 protocol tests passed**, including the exported UI
  interactions, split dialog, drag cancellation, collection placeholders, stack
  replacement IDs, membership/overflow packets and applied corrections. The bag
  layout regression verifies 28-slot boundaries, tier artwork ordering, premium
  gating, overflow-only paging and refusing pointer actions on loose items.
- `python -m unittest discover -s .local/dso_server/tests -p '*inventory*.py'`
  with `.local` on `PYTHONPATH`: **8 tests passed**, covering merge limits,
  quantity/value conservation, allocation/identity changes and rejection paths.
- Native release build succeeded. Its existing unused `Label.bold` warning
  remains; test builds also report two existing Bevy deprecation warnings.
- Six native renderer captures were visually inspected under
  `.local/logs/inventory-layout/`: normal, ingredient tiers I and V, premium,
  expired premium and overflow. They confirm four normal/premium rows, visible
  tier buttons and changing collection quantities, a disabled expired premium
  tab, and paging shown only for loose items. The optimized executable was
  rebuilt successfully. Desktop input/screen-grab automation was unavailable;
  interaction behavior was checked in the UI and server tests rather than
  against a live account.
- All 393 ingredient entries have icons. Exported inventory/tooltip/split
  layouts load. The known missing `assets/characters/stats.json` error remains
  separate from this inventory work.
- The server patch passed an application check against saved original files;
  `git diff --check` passed. Detailed test/build logs are under `.local/logs/`.

## Equipped character appearance

The inventory work was committed and pushed as `1fa3082`. Appearance work now
resolves local equipment from the retained authoritative inventory, including
compact `ItemUpdate` changes. Remote spawn and update packets keep their skin
parts and colour. Updates are coalesced per actor and applied after spawning;
unchanged equipment keeps the current skeleton. Redressing preserves the
logical animation state and unrelated character attachments.

`tools/equipment_visuals.py` follows the server's resolution order: runtime
transmog or appearance ID through `_Template_ItemAppearance` and
`_Template_EquipmentSkin`, then the item's `Skin` or `EquipmentSkinId`. The local
release261 export contains 3,321 item mappings and 2,661 appearance mappings.
The old flat table format remains readable. Inventory UI exports regenerate
this table with their selected database; character exports accept `--database`.
To regenerate only the equipment mappings:

```powershell
python tools/equipment_visuals.py .local/dso_server/data/db/static.db4 assets
```

Visible slots 0-7 select the class skeleton's meshes and female variants where
available. Semicolon-separated graphics are expanded; two-handed meshes are
drawn once. Removing equipment restores base clothing. Missing converted
equipment meshes leave the base clothing available. Dyes clone each mesh's
material and use the existing shader's authored green-channel dye mask, so
another player wearing the same model keeps their own colour.

The installed graphics pack still lacks at least one referenced mesh for 40
wearable item templates (plus 116 non-slotted entries). Updating lookup data
does not create those missing graphics. The local audit details are in
`.local/logs/appearance-assets.json`; matching converted meshes are required
for those skins. The fixture outfits used below have all their meshes and masks.

The remote wire field historically called `armament` is a player-kind byte in
release261 (the server sends 5). Remote poses now use the advertised animation
set, with hands as a fallback on spawn. Rangers use their large-weapon set only
for two-handed equipment; local poses follow authoritative hand allocations.

Validation: **47 client tests and 2 Python export tests passed**. The new
tests cover lookup precedence, cached item records, compact transmog/dye updates,
unchanged-outfit preservation, attachment preservation, gender variants,
two-handed deduplication, ranger and remote weapon poses, dye isolation, and
server-encoded spawn/update/unequip batches. Fixtures are generated by
`tools/appearance_vectors.py`.

The optimized native executable built successfully. Three captures under
`.local/logs/appearance/` were visually inspected: `initial.png`, `changed.png`
and `unequipped.png`. They show the local male and remote female outfits,
independent red/green dyes, a blue local transmog with the remote female model
variant, and restored base clothing after unequipping. Skin diagnostics reported
no dead or foreign joints. Existing missing `characters/stats.json`, optional
debug-map sidecars and a zero-size text-span warning remain unrelated.

Explicit `--cam` coordinates are now respected during offline play, allowing
these close-up captures. `DSOR_INVENTORY_PREVIEW` accepts the appearance fixture
batches as well as ordinary inventory packets. Packet-driven ECS tests exercise
updates on existing entities; captures replay the final fixture states. No live
account or multiplayer session was modified. `git diff --check` passed.

The appearance changes were committed and pushed as `c151885` to
`origin/release261` on 2026-10-14.
World drops remain deferred.

## Smooth equipment transitions

The reported weapon/shield flicker came from rebuilding the complete skeleton
on every outfit change, resetting playback, and starting the incoming animation
at full weight. The current fix retains the skeleton, animation player, graph,
bone attachments and unchanged body/clothing meshes. Replacement parts load
hidden; the old outfit remains visible until its replacement and textures are
ready. Readiness checks the live scene instance, meshes and material textures
(including dye masks). Native testing exposed a Bevy scene whose recursive
dependency status remained `Loading` after its assets were present; the handoff
therefore checks the actual render assets instead of that aggregate status.
Publishing the new outfit also selects its weapon animation set.
`DSOR_CHECK_SKIN=1` also reports pending outfit loads.

Weapon posture changes blend over 220 ms. The incoming clip starts at its
existing weight (zero for a new clip), and outgoing weights decrease while the
total stays at one. A second change during the blend starts from the current
mixed pose. Equivalent movement and one-shot states retain normalized clip
progress across sets; common clips continue without restarting. Ordinary state
changes retain the existing 150 ms blend duration.

- [x] Preserve the live skeleton and defer replacing equipment until it is ready.
- [x] Blend weapon poses with normalized weights and retain animation progress.
- [x] Exercise real animated joint rotations, rapid reversal, shared clips,
      one-shot progress, and retaining the old outfit during loading.
- [x] Cover waiting for base, normal and dye textures before publishing outfits.
- [x] Final client regression run: **51 tests passed**.
- [x] Final optimized native build succeeded.
- [x] Three timed local/remote renderer runs, with 181 captured frames each.

All three final replays completed six equipment updates during capture, including
a rapid reversal, two-handed weapons, empty hands and sword/shield restoration.
Key transition frames were visually inspected. Both characters retained their
skeletons throughout the updates; there were no skin-binding warnings or stalled
outfits. Logs and frames are under `.local/logs/equipment-transition-verified1/`
through `equipment-transition-verified3/`. The final test/build logs are
`.local/logs/equipment-blend-tests-final.log` and
`.local/logs/equipment-blend-build.log`. `git diff --check` passed.

`tools/equipment_transition_preview.py` generates an initial server packet and
six timed equipment updates without opening an account. On the debug map,
`DSOR_INVENTORY_PREVIEW=<output>/initial.bin` supplies the initial outfits and
`DSOR_EQUIPMENT_REPLAY=<output>/sequence.json` schedules updates by frame after
both initial outfits are ready and the loading screen closes. Use `--play 0 0`
and the existing `DSOR_SHOT_BURST` capture option to inspect the transitions.

The transition fix is included in the character-selection and equipment update;
the earlier appearance commit is `c151885`. World drops remain deferred.

## Clothing and body coverage

The default torso item was already replaced when a torso was equipped, but
the client still drew every base body segment. Those segments protruded through
the replacement armor. The new selection step uses the original
`tables/gfxskintags.bin`: each named mesh has a region mask and layer. Only
visible higher layers hide matching lower layers; equal layers coexist.
This applies to local and remote players, including base clothes, boots,
gloves and transmog. The male/female mesh is resolved before its mask is used.
Authored NPC outfits retain their explicit part lists.

Coverage is calculated for the entire desired outfit, then published through
the existing atomic outfit swap. Removing equipment restores the newly exposed
body segments and default clothes. Missing converted equipment cannot hide the
fallback outfit; unknown coverage is left visible conservatively.

- [x] Read and export the original version-2 coverage table.
- [x] Regenerate both local part catalogs (1,714 human and 905 dwarf rules).
- [x] Cull lower layers when assembling player outfits.
- [x] Add regression cases for gender-specific cuts, body restoration,
      unavailable armor, equal layers, hidden occluders and legacy catalogs.
- [x] Two Python exporter tests passed.
- [x] Final client regression run: **54 tests passed**.
- [x] Optimized build and native before/after captures.

The human before/after captures show the protruding base geometry removed on
both local male and remote female characters, while exposed skin remains
visible. The initial scene drops from 34 to 22 rendered character surfaces.
Final human and dwarf replays each captured 181 frames and completed all six
equipment updates. Visually inspected frames cover different torso cuts,
removing boots/gloves, restoring default clothing and re-equipping the outfit.
Both characters retained their skeletons, with no new skin-binding warnings or
recurring outfit waits. The dwarf replay briefly retained the old outfit while
the replacement torso loaded.

Capture logs and frames are under `.local/logs/clothing-coverage-before/`,
`clothing-coverage-after/` and `clothing-coverage-dwarf/`. The final test/build
logs are `.local/logs/clothing-coverage-tests.log` and
`.local/logs/clothing-coverage-build.log`. `git diff --check` passed.

Regenerate older asset packs with `python tools/skin_coverage.py <export_win32>
<assets>`. The full character-table export also includes this step. The native
regression fixture can be generated with
`python tools/equipment_transition_preview.py <server> <output> --clothing`;
it swaps torso models, boots and gloves on local male and remote female players,
then restores the base outfit. The coverage changes are included in the
character-selection and equipment update.
World drops remain deferred.

## Character selection scene

The previous client automatically selected the first roster entry (and created
a default character on empty accounts). The character service now stays open
until the player chooses **Play Now**. The selection screen uses the converted
`charselectionwindow.bxml` geometry, textures, button states and roster-row
template, together with the original `a0000_char` forest scene, fire, fog and
lighting. Camera pitch/yaw, offset and FOV come from `_Globals.CharGen*` in
`static.db4`, exported as `characters/selection.json`.

- [x] Preserve full roster and operation-9 preview equipment replies.
- [x] Request operation 8 when browsing; send operation 3 only on Play Now.
- [x] Clone four XML roster rows, page additional characters, support keyboard
      selection, drag rotation and the original rotation buttons.
- [x] Resolve sparse equipment slots from `_Template_Item.SlotType`, preserving
      transmog, dye, gender variants, coverage culling and class proportions.
- [x] Resolve roster hair/beard indices and use `CharSelectIdle` animations.
- [x] Keep the previous portrait until its replacement outfit is ready.
- [x] Gate gameplay input/HUD until the actual game map loads.
- [x] Add name/class/gender creation and explicit deletion-confirmation dialogs.
- [x] Native rendering: all four classes, both genders, five equipped previews,
      portrait rotation and live-server selection-to-Kingshill handoff.
- [x] Final client regression suite: **67 tests passed**, including XML input,
      hidden-preview animation readiness and preselected roster scrolling.
- [x] Protocol regression suite: **11 tests passed**.
- [x] Python exporter/fixture compilation and `git diff --check`.
- [x] Final optimized executable build and capture check.

The roster's historical codec field `free` is release261's total
`character_slots`; remaining slots are calculated from the account roster.
Repeated initial rosters preserve selection and newer equipment replies; late
preview replies are cached by character ID and cannot switch the active model.
Character creation currently uses a compact dialog; recreating the separate
`chargenwindow.bxml` customization screen is outside this selection-screen work.
Purchasing slots, instance switching and paid appearance editing remain deferred.

`python tools/export_character_selection.py <static.db4> <assets>` regenerates
the camera/cosmetic/slot catalog; `character_tables.py` also runs this export.
`python tools/character_selection_preview.py <server-package> <output.json>`
creates a five-character fixture using the real server packet encoder. Run
`dsor-client a0000_char` with `DSOR_SELECTION_PREVIEW=<output.json>` to exercise
the roster without opening or changing an account.

Native regression captures are under `.local/logs/character-selection-verified/`
and `.local/logs/character-selection-online/`. The online run used the real
Python services on a separate loopback address with disposable player stores:
it browsed all five entries, returned to the first, rotated the preview, then
explicitly played the second character into Kingshill. The selection UI and
portrait were removed on handoff; the normal world camera and HUD returned.
No account on the normal server was changed.

`DSOR_SELECTION_REPLAY=<steps.json>` optionally automates this native check.
Steps are an array of objects with a `frame` and any of `select` (character ID),
`rotate` (radians), `capture` (PNG path), `play: true` or `quit: true`. Frames
advance only after the map and current portrait have loaded. Normal sessions
do not replay actions. The input regression separately drives the actual XML
hit regions through pointer selection, keyboard navigation, list scrolling,
resizing, rotation and deletion confirmation. It caught an authored overlap
between the portrait drag area and rotation arrows; explicit buttons now win
that hit test. Preselected characters beyond the first four rows scroll into
view automatically.

Final build and regression logs: `.local/logs/character-selection-build-final.log`,
`character-selection-client-tests-final.log` and `character-selection-protocol-tests.log`.
The rebuilt executable passed both native replays (seven offline captures and
eight live-service captures), including the fifth-row scroll and world handoff.
World drops remain deferred.

## Upstream release261 integration (2026-10-15)

Integrated upstream `release261` through `af0252e`: spell-effect timing and
materials (`7264583`), decorative town characters and sequences (`b4d1266`),
and asset-conversion documentation (`af0252e`). Upstream's literal `main`
remains at `9f3dd83`, already in this fork; the recent work is on `release261`.

Git merged the overlapping README, NPC, monster and exporter changes without
text conflicts. Character selection, inventory behavior, equipment pose
transitions, clothing coverage, English UI and the local-map overlay are
preserved. The merged client regression suite passed all **67 tests**.

Regenerated skills, decorations, monsters, material animations and stats in the
local asset pack. This exposed two exporter compatibility problems, now fixed:
release 261 uses `db.skill2.xml` for skill captions, and Windows' default text
encoding produced unreadable monster JSON or failed to write stats. The stats
exporter accepts both skill-caption filenames; both exporters write UTF-8.
The resulting catalogs load successfully (7,964 monsters and 200 level curves).

The optimized build and live-service replay passed: all five equipped previews,
rotation, paging, and entry into Kingshill with the selected second character.
Kingshill loaded 205 decorative characters and 34 effect sequences. The asset
exporter generated decoration files for 211 maps; it skipped 38 character tracks
whose clips are absent from this local asset pack. Existing optional sidecars
for the selection map remain absent. No new skin-binding warnings or panics
were observed.

Validation logs are under `.local/logs/upstream-release261-merge-*` and
`.local/logs/upstream-merge-*`; final native captures are in
`.local/logs/upstream-merge-online/`. `git diff --check` passed.

## UI shortcuts, defaults and window stacking (2026-10-15)

- [x] Bind the skills/talents window to **S**; focused text fields still consume
      shortcuts.
- [x] Initialize wisdom level, progress and available points to **0** in both
      the HUD and talents window. Replace the XML's sample available-points
      text and initialize labels that load after character values settle.
- [x] Reduce shared UI fonts by **20%**, including runtime item counts/tooltips,
      map captions and character-selection dialogs.
- [x] Keep each menu's artwork, text and runtime items in a separate depth band.
      Newly opened windows and clicked windows come forward as a whole; map,
      modal and tooltip layers retain their intended priority.
- [x] All **68 client tests passed**, including real exported layouts, populated
      inventory rendering, repeated reopening, focused text input, wisdom bars
      and late-loading labels.
- [x] Optimized native build passed. Inspected the skills window and travel map
      over a populated inventory: background artwork, item icons and stack
      labels stay below the front window, and reduced text renders correctly.

Wisdom progression beyond its default still needs server packet decoding.
Validation: `.local/logs/ui-order-tests.log`, `ui-order-build.log` and native
captures under `.local/logs/ui-order/`. `git diff --check` passed.

### Follow-up: contents disappearing after focus changes

The initial overlap captures did not exercise focus changes. Bevy 0.19 retains
the sort key of a transparent `Material2d` mesh when only its transform changes,
while sprites and text sort from their current transforms each frame. Lowering
a window could therefore leave its background drawing over its own contents.

- [x] Mark moved UI meshes for render-order refresh after transform propagation
      and before Bevy checks mesh changes. Stationary windows retain their cache.
- [x] Extend the real-layout regression to check Bevy's pending mesh updates,
      including both raised and lowered windows.
- [x] All **68 client tests passed**, including the retained-order regression.
- [x] Native replay: seven captures, six verified focus changes and a title
      drag from side-by-side windows into overlap. Twenty image-region
      comparisons confirm that exposed labels, icons, equipment slots and stack
      counts remain unchanged (allowing one byte of blend rounding).

Final executable: `target/release/dsor-client.exe`. Logs:
`.local/logs/ui-focus-tests.log`, `ui-focus-build-final.log` and
`ui-focus-pixels.log`; captures, entity snapshots and pointer fixture:
`.local/logs/ui-focus-verified/`. `git diff --check` passed.

## Indoor ambience and player visibility (2026-10-15)

- Adjusted the entrance faces of Kingscity's church and palace ambience boxes
  through persistent level overrides, and applied those transforms to the local
  ambience asset. The approach stays outdoors until the doorway/room entrance.
- Evaluate bubble membership after player movement propagates. Normalize sun
  directions before blending so a large volume does not accelerate the fade.
  Preserve disabled bubble flags in the conversion exporter.
- Cast five camera-to-player rays against map geometry. Obstructions receive a
  local feathered opening with fade/hold timing; floors, geometry behind the
  player, actors and shared materials are preserved. The depth/normal prepass
  matches the cutout, and buildings keep their original shadows.
- All **79 client tests** and **7 lighting/export Python tests** passed. The
  optimized native build passed; eight captures verify approach, entry, interior
  and exit at both locations. Both exits restore all affected material handles,
  and the replay logged no renderer errors or panics. Browser rendering was not
  exercised in this run.

Executable: `target/release/dsor-client.exe`. Validation:
`.local/logs/indoor-tests-final.log`, `indoor-build-final.log`,
`indoor-runtime-verified.err`; captures in `.local/logs/indoor-captures/`.
See `docs/fx.md` for the cutout behavior and offline replay format.

### Follow-up: reveal only a fully occluded character

Cutouts now require blocked rays to all five body samples: head, torso, lower
body and both sides. Tables, chests and narrow pillars keep their materials when
any sample remains visible. Separate meshes can still jointly hide the whole
character; exposing one sample closes an existing opening with the usual fade.

All **83 client tests passed**, including real-mesh regressions for low furniture,
narrow pillars, combined blockers and restoring the scene when one side clears.
The optimized native executable was rebuilt. Eight Kingscity replay captures
confirm full wall coverage still reveals the character and both porches/exits
have no active cutouts; the palace porch keeps its partially blocking wall intact.
The replay reported no runtime errors.

Validation: `.local/logs/full-occlusion-tests.log`, `full-occlusion-build.log`,
`full-occlusion-runtime.log`; captures in `.local/logs/full-occlusion-captures/`.

## Loading barrier, class preload and render warm-up (2026-10-19)

- Map loading now waits for a scene-readiness barrier besides geometry and
  props: navigation and ambience files, nearby NPC/decor scheduling, every
  spawned scene's live meshes, materials and textures, first animation poses
  and the player's complete outfit. Failed optional scenery settles with the
  asset server's diagnostics; a failed player appearance keeps the error and
  the close button visible.
- The selected class's skill, status, hit and summon effects (including the
  summons' selected skin variants) are planned from the skill, status,
  sequence, monster and outfit catalogs and requested four at a time once the
  initial geometry and props have settled. The handles stay resident for the
  class, now also through a map change: every actor is despawned before the
  next map's player arrives, and the first version dropped the handles in that
  gap and reloaded all of them on every transition (regression test added).
- Behind the opaque overlay, bounded batches of inert mesh/material samples
  (16 proxies and at most two glTF scene descriptions per batch; particle quads
  with the live particle material, decal quads, skinned proxies with
  placeholder joints) are drawn through the real game camera. The render world
  acknowledges uploaded meshes, prepared materials and compiled pipelines over
  two frames before a batch is removed; a failed pipeline or a batch over
  90 seconds fails the loading screen with a message. Nothing plays skills,
  emits particles or triggers audio.
- Progress: 75% map models, 15% class assets, 10% warm-up. `loading readiness:`
  is logged every five seconds with the current stage, `preload:` and
  `render warm-up:` report counts, and `loading screen: ready` closes.
- All **296 client tests passed** (14 ignored for missing converted assets),
  including the preload, warm-up, readiness and loading tests. Two
  pre-existing failures were fixed on the way: the graphics settings test
  still assumed vignette on by default, and the raknet replay test panicked on
  Windows without `HOME` instead of skipping.
- Native validation with the fast build `.local/preload-client.exe`
  (`.local/Build-Preload.ps1`, DLSS feature; `DLSS_SDK`, `VULKAN_SDK` and
  `LIBCLANG_PATH` point into `.local/dlss/`): mage and dwarf in Kingscity and
  the mage on `a0007_pastures_pve` (summons) all reached `loading screen:
  ready` (80/69/80 preloaded assets, 603/267/603 warm-up samples, 4-8 s from
  map start, no renderer errors or panics).
- One smoke surfaced a shader regression from the fog commit: `ambience.wgsl`
  read `mesh_view_bindings::fog`, which Bevy declares only for views carrying
  `DistanceFog`. The hover highlight camera has none and draws the hovered
  actor's own materials, so those variants failed to compile for that view and
  no outline could be drawn. The fog helpers are now guarded by `DISTANCE_FOG`.
  A hover replay (`.local/smoke_hover.py`: cursor on nearby NPCs with
  `renderaudit`) shows the highlight camera active in six audits with zero
  shader errors.

Logs: `.local/logs/preload/finish-1`, `finish-3`, `finish-summon-1`, `fixed-1`
and `hover-1` (`.log`, `.png`, route and UI replay files). The earlier runs
`warrior-0`, `ranger-2`, `dwarf-recheck-3`, `outdoor-summon-1` and
`final-native-1` are in the same directory.

## Dragged item frame and destroying items (2026-10-19)

- A dragged item now carries its frame (`icons_toolkit/frame_slim_*`) over its
  rarity background and icon, as its cell draws it: cells, the drag cursor and
  the new dialog share one helper (`ui::controls::items::sprites`).
- Dropping a carried item (bags or premium bag) out of every window, on the
  world, opens `confirmdialog` in its centred `DeleteItem` variant: the item in
  its slot (with its stack count), "Do you really wish to destroy this item?"
  and its name in its rarity colour. Confirm sends `InventoryCommand` (0x55)
  operation 0 with no arguments; the item stays until the server's
  `InventoryInfo` delta/final pair removes it (DSOR_Server
  `services/inventory_map.py`, the discard branch). Escape, Cancel, or the
  item leaving the bags closes the dialog. Equipped items are not offered:
  the server's discard leaves their look and stats in place. Dropping over a
  window, or while trading, does nothing.
- `DSOR_TEST_DESTROY=1` (offline, with `DSOR_INVENTORY_PREVIEW`) opens the
  dialog on the first carried item for captures.
- All **303 client tests passed** (14 ignored for missing converted assets;
  the 13 asset tests also pass locally), including four new destroy tests.
  Captures: `.local/logs/destroy-dialog/dialog.png` and `inventory.png`.

### Follow-up: DLSS / DLAA in every native build (2026-10-19)

The launched executables (`target/release` and the live client's `fast` build)
had been rebuilt without the opt-in `dlss` Cargo feature, so Settings showed
"NVIDIA DLSS / DLAA: Not in this build". The feature is now a default feature
of the client crate; its Windows/Linux-only dependency keeps browser builds
unaffected. A tracked `.cargo/config.toml` supplies `DLSS_SDK`, `VULKAN_SDK`
and `LIBCLANG_PATH` as paths relative to the workspace (`.local/dlss/`), with
environment variables taking precedence, so a plain `cargo build` includes
DLSS. CI builds the client with `--no-default-features`. The local build
scripts copy `nvngx_dlss.dll` beside their executables.

Verified: a type-check with no SDK variables in the shell; the release build
(9 m 38 s, thin LTO) and the live client build both compile
`rendering/graphics/dlss.rs` and carry the NGX symbols; the release executable
logs `DLSS / DLAA: available` on the RTX 4070 and reaches `loading screen:
ready` (`.local/logs/preload/release-dlss-1.log`); all **306 client tests
passed** (14 ignored). Every native build now initialises Vulkan; build with
`--no-default-features` for another backend.

## Server-to-client wiring: merge, vendors, party, guild, quests, travel, talents, logout, currency, traps, systems (2026-10-20)

The release261 server (DSOR_Server, `.local/dso_server`) already handles most of
what the client never sent; this pass wires the client to it, with every layout
checked byte for byte against the server's own codecs
(`tools/vendor_vectors.py`, `social_vectors.py`, `misc_vectors.py`,
`systems_vectors.py` write `crates/proto/tests/data/*_release261.json`; the
`crates/proto/tests/*_release261.rs` tests decode, round-trip and encode them).

- **Two server trees.** The live server (`python run.py`, UDP 2190) runs from
  `D:\_dso_retired\dso-server\dso_server`, which has the trade service and the
  2026-10-19 combat changes but not the stack merge; the git clone
  `.local/dso_server` has the merge (ops 0x0D-0x10) but had no trade. The trade
  service was ported into the clone as new files (`protocol/trade_protocol.py`,
  `services/trade.py`, its test) plus the dispatch and mixin lines; a
  handler-level merge test was added (`tests/test_inventory_merge.py`,
  `MergeHandlerTests`). The clone's suite: **1344 passed, 3 xfailed**. The
  retired tree was left untouched: restart the live server from the clone
  (`.local\Run-Online.cmd`) to get the merge, or port those files.
- **Stack merge** needs no client change: the client's InventoryCommand
  (`u32 item, u8 op, u32 count, args, u32, u32`) is what the server decodes.
- **Vendors.** `inventory::Transaction` is now the server's layout (counted
  entries of `bool, str offer, u32 item, u32, u32 amount`, then `str shop`,
  `bool`): `Transaction::buy/sell/restore`. The trading screen's offer cards are
  buttons (one copy per click), the buy-back cards restore, and a right click on
  a carried stack while a shop that buys is open sells it (the server's buyback
  list comes back as an Offer that keeps the shop). Layouts for the rest of a
  purchase's reply: QuickbarInfo 82, TimedOffersInfo 130, GameCurrencyChanged
  310 (kept in `Net.game_currencies`), the tier-spender progress 314.
- **Party.** `social::GroupMember` follows the server's PlayerSummary (role byte,
  note string); `GroupStatus` is kept in `Net.group` and drawn as party frames
  (`ui/controls/party.rs`, the `groupwindow` template: a row per member with
  name, level, leader mark and a health gauge, `Gauge::Group(n)`, from the
  actor's last known vitals; members elsewhere are greyed). Leave and found
  (socialscreen's buttons) send GroupLeave 160 / GroupFoundation 152; the
  foundation response, GroupingFailed 163 and the invitation answers reach the
  chat. The server auto-creates a group on an invite, so founding is optional.
- **Guild.** A kind-3 invitation opens the confirm dialog and answers with
  GuildInvitationResponse 171 (0 joins, 1 declines — the server refuses the
  notification codes 1/2). `player::GuildStatus` decodes the members and the
  IgnoredStatus 179 that follows; found/leave send GuildFoundation 168 (the name
  typed in the social window's search field) / GuildLeave 173.
- **Quests.** The log keeps per-task counters (`gameplay/quests.rs`:
  QuestLogInfo's finished mask and counters, QuestUpdate kinds 0/1); the quest
  log window lists the active quests with level, state and a cancel button
  (QuestCommand operation 2, only for quests.xml-deletable quests) and shows the
  open quest's text and objectives with "x/y"; the tracker
  (`activequestwindow`) follows one quest with previous/next. Task titles come
  from `interface/quests.catalog.json` (`tools/quest_catalog.py`, from the
  server's quests.xml, in `convert_assets.sh` with DSOR_SERVER) and the
  dialogue catalog's `quest.<quest>.s_<task>` texts. Delivery options (NPCInfo
  kind 3) appear in the NPC menu and send Deliver 104; a left click on a prop
  the server announced walks to it and sends PropInteraction 57
  (`world/props.rs`). QuestTriggerSignal is not sent: the server detects
  trigger volumes from movement itself.
- **Travel.** `player::Travel` is the native `str source, str map, str entry,
  bool`; the travel map's destination buttons send it (an empty source and
  entry is the selection the server reads first; it requires the map
  discovered). A right click on a travel item sends its use (0x13) then
  UseTravelItem 121 with the item's map (`travel_items` of the quest catalog,
  from `_Template_Item.TravelItemTarget`). Event portals (event-set exits) are
  still not drawn.
- **Talents.** SelectTalent 279 carries the rank delta; a talent card buys one
  rank, the preset buttons send EnableTalentPreselection 281; reset commands
  286/287 and the points/preset info 282/285 have layouts, the selection and
  points are kept in `Net`. The window does not draw the ranks yet.
- **Logout / switch character** (gamemenu) sends LogoutCommand 99; the
  countdown (PlayerLogoutInfo 327) is told in the chat. The server has no other
  logout than the character screen. Delete character was already wired.
- **Currency.** Badges of honour, draken and glyphs reach the currency bar
  (honor_display, event_display, cr_display); HonorPointsChanged 127 updates
  the honour; keyed currencies (materi, event coins...) are kept.
- **Dwarf traps.** NewTrap/TrapTripped/DiscardTrap drive a looping model per
  trap template (`gameplay/skills.rs trap_visuals`: dwarf_mine* ->
  effects_dwarf/dwarf_tripmine, the ranger trap, the fox trap) and the
  `dwarf_tripmine_explode` sequence on a trip.
- **Items.** A right click identifies an unidentified carried item (operation
  0x12). Layouts (not yet in a window): Salvage 49, Crafting 92 and its result
  93, RecipeBookInfo 311, GemRemovalResponse 86, ApplyItemCustomization 322 and
  ItemCustomizationInfo 320, the stickerbook's 303/304/305/307/308 (the album's
  entries are kept in `Net.stickerbook`).
- **Not wired, decodable:** events (ActiveEvents 222, SetEventParticipation
  120, the progress/participation infos), dailies (131/148/149), the Wanted
  Thief (356-359), titles (SetTitle 277), the rankings (requests 235/240/242/
  244, pages 236/241/243/245), PvP (the queue, battleground and match commands;
  `JoinMatchQueue` is now `u32 tick, i8 mode, i8 kind`), the premium shop's OCP
  (267/271/313; `OcpOffer` gained its `featured` bit). Their answers are kept in
  `Net` and the match-making ones are told in the chat; their windows are
  unchanged.
- **Trade** is the server-side port above; the client's trade window matched
  the server's `trade_protocol.py` ids (0x0F00-0x0F05, 0x0F10) already.

Verified: the protocol crate's **35 tests passed** (five new vector suites: 3
vendor, 4 social, 2 misc, 4 systems, over 82 server-generated vectors); all
**321 client tests passed** (14 ignored for missing converted assets), with
new tests for the party frames, the quest log and tracker, the merchant's
card actions and the quest entries; the server clone's suite: 1344 passed.
Not tried in the running game: the live server runs from the retired tree
(see above), so the merge, trade and the new windows await a restart from the
clone and an online session.

## The systems' windows (2026-10-20)

The windows the previous pass left unwired draw their data now, each an
authored template cloned per entry (`client/src/ui/controls/template.rs`:
`clone_at`, `set_text`, `icon_layers`, `selected`, `dim`, `without`; the party
frames and the quest log use the same helpers). `controls/systems.rs` builds a
`Views` resource every frame from `Net` and the systems catalogue
(`assets/interface/systems.catalog.json`, `tools/systems_catalog.py` in
`convert_assets.sh`: achievements and their categories, events, daily
challenges, Wanted missions, the album's books and stickers, recipes, skins,
socket prices, texts resolved from the language files) and rebuilds only the
windows whose view changed. A rebuilt window keeps its pages, toggles and texts
(`WindowControls::carry`; before, a rebuilt window kept its *old* buttons). The
clones' buttons carry `Sys:<kind>:<arg>` events, parsed into
`systems::Request`, queued by `interact` and sent by `systems::send`, which
also asks the server what an opened window needs (OcpOfferRequest,
ActiveEvents, WantedThiefInfoRequest, the ranking requests).

- **Leaderboards.** The PvP inlay lists the overall board (first category tab,
  OverallRankingRequest 240) or the honour board (HonorRankingRequest 235,
  board 4 — the only one the server records; class tabs filter), ten rows a
  page (the scroll buttons page, ScrollToMe asks for the player's page); our
  own rank (242/244) feeds the matchmaking window's standing.
- **PvP queue** (matchmakingwindow): the text tells the queue's state; Join
  battle joins the open tab's mode (JoinMatchQueue 206: 1v1, 5v5), leaves it
  (AbortMatchQueue 209) or accepts the match found (JoinBattlegroundResponse
  211).
- **Premium shop**: the server's Andermant offers (OcpOfferResponse 270) as
  cards on the deals and offers pages; Buy sends OcpBooking 271 (product key,
  method "external").
- **Events**: a tab per running event (EventUpdate 221), the open one's title
  and text, the time left (ends in `date`), its progress (224), a teleport to
  its area (the catalogue's TeleportTarget, Travel 93). EventUpdate is
  incremental (merged by event id); map-local entries, ended events and the
  Silent, Sale and HighlightSale categories get no tab, as in the original
  (`systems::events::shown`, since 2026-10-23).
- **Collector's album**: a grid per category page (the front page lists the
  favourites), owned stickers bright and usable (UseStickerBookItem 309),
  the others faded with Add when a carried item fills them (307), favourite
  (305) and upgrade (308) buttons.
- **NPC desks** open from the NPC menu by profession (jeweller: gem desk;
  smith: salvage and workbench; crafting: workbench; tailor: transmog) and take
  a right-clicked inventory item (`items.rs` → `systems::pick`): the salvage
  desk melts up to nine picks (Salvage 49); the gem desk shows the item's
  sockets, sets a right-clicked gem into the first free one (InventoryCommand
  0x11 [item, socket]), takes one out (0x14 [socket]) and buys the next socket
  (0x15, priced from `_Template_SocketExtensionCosts`); the workbench lists the
  known books' recipes by the crafted item's category and crafts the open one
  with the carried ingredients (Crafting 92, gold or Andermant); the transmog
  desk lists the skins fitting the worn item picked for our class and applies
  the chosen one (ApplyItemCustomization 322; slot type 12 for the cloak).
- **Quest log pages**: achievements (categories with the open one's
  sub-categories, rows with progress and points from AchievementInfo 276, the
  earned title), dailies (four cards: Play accepts, Shuffle replaces,
  DailyChallengeSelection 148), the Wanted board (four cards: accept 358,
  refresh 359, the bottom button refreshes all; "not unlocked" until 357 answers).
- **Titles** (charactersheet): the titles our completed achievements unlock as
  rows (SetTitle 277 kind 1), the radio buttons for none (0) and the PvP rank
  (2).

Verified: **336 client tests passed** (14 ignored), with unit tests for every
window's `prepare` and for the request parsing; the systems catalogue
round-trips through its loader. Not tried online (the live server runs from the
retired tree); a release build with `DSOR_UI_OPEN` renders the empty windows.

## The systems' windows on their 2026 layouts (2026-10-20)

The previous pass drew the systems' windows, but most of them looked wrong:
backgrounds over their rows, artwork cut from the wrong part of its texture,
buttons that did nothing, edge panels not anchored, and two windows built on
legacy or invented layouts. Each window now follows its 2026 layout and every
button it shows either acts or shows its disabled face.

- **Anchoring.** A window's root Canvas lays its children against the whole
  screen (left `k` 0, right 1, centre 0.5). `tools/export_ui.py` exports that
  and `tools/patch_ui_anchors.py <fresh assets> <assets>` copied only the new
  `k` values into the converted layouts (35 windows: the quest tracker sits at
  the right edge again, the character sheet and workbench at the left).
- **Text outlines.** Labels with `TextEffect 1` (about 4,000 of 7,500) carry an
  outline in the export; the client drew none, so gold captions on light
  buttons were unreadable. Each outlined label now has four copies behind it
  in the outline's colour, each with a `Text2dShadow` on the opposite side:
  eight directions at the outline's radius (`ui::outline_copies`);
  `ui::follow_outlines` keeps them on the label's text, size, wrap width and
  alpha.
- **Draw order.** A button state's caption is drawn over the state's own
  artwork (its relief shading and frame follow the caption in the layouts):
  `ui::draw_order`. The quest log's achievement rows and overview put their
  backgrounds under their content (the game's controller does the same).
- **Quest log and tracker.** Objectives show an empty or ticked box
  (`icon_checkmark_empty` / `icon_checkmark`); long objectives wrap (34
  characters in the tracker, 60 in the log) and the tracker grows with them.
- **Achievements.** Rows show their icon, title, text and points; the
  overview shows each category's title, progress bar and count, scrolls to the
  points entry, and the recent achievements' panel shows again.
- **Daily challenges and Wanted.** The cards sit in their slots
  (`TemplateSlot0..3`, the template's left edge at each), the challenge
  artwork keeps the authored crop, the shuffle shows its price (40 Andermant,
  `_Globals.DailyAchievementReshuffleCosts`), the bonus bar shows the seven
  bonus rewards with how many dailies each needs and a tick once reached, and
  the next one. The Wanted bar shows the time to the board's reset (midnight
  UTC) and the refresh price; the privileged poster shows its count and opens
  the membership page.
- **Events.** The bar shows a page of rewards at a time (their page in
  `EventRewardItems "[page/]threshold:item:amount"`; the unpaged form was
  dropped before), evenly spaced with the fill rising between thresholds, the
  Andermant rewards as marks; Prev/Next turn the pages. The useful offers
  (`EventOffer`, the Andermant shop's) fill their slots and buy on a click; the
  collectible's icon sits by the count.
- **Collector's album.** Rebuilt on the authored `Categorypage` and category
  list: categories with their books under the open one, the front page with
  its banners (counters switch them), the new stickers and the favourites;
  a book's page with its text, a 6 x 4 grid, the preview (name, origin,
  background, favourite flag, its offer) and the book's progress with its
  rewards (`StickerbookRewardItems`). A click previews a sticker, a right
  click uses it. The exporter no longer adds the invented `AlbumPage` /
  `AlbumCategory` widgets (`tools/collector_catalog.py` is gone).
- **Transmog desk.** The 2026 customization window: skins and dyes tabs, the
  eight slots (the worn item, a reset once it has a skin or dye; a click picks
  the slot), a 5 x 4 skin grid (effect, shop, owned and worn flags) or the dye
  grid, owned and shop filters, the collection bar, prices as the server
  reckons them (`_Template_DynamicLevelCosts`; a carried dye is free) and
  Apply (disabled until something would change). The eye toggles have no
  server command and are hidden.
- **Gem desk.** `gemdeskwb` (ten sockets) replaces the legacy `gemdesk`;
  "Open Workbench" opens the workbench.
- **Workbench.** The authored tabs (their icons), the category tree with the
  known and the locked recipe books, the filters, the book's text and usable
  items, its recipe's ingredients with their amounts (`"template:amount"`;
  before, the amount suffix stopped every craft), result, prices, the bulk
  amount and Craft (gold or Andermant, disabled without the ingredients). The
  crafting animations' smoke no longer covers the slots.
- **Shop and membership.** Offer cards show their amount; the membership
  page's shop buttons open the shop.
- **Catalogue.** `tools/systems_catalog.py` adds the daily and Wanted figures,
  event pages, currency rewards, collectibles and offers, album rewards,
  origins, backgrounds, offers and banners, the skins' details, the dyes and
  the level price factors.
- **Fixtures.** `tools/ui_fixture_packets.py` writes the server's packets for
  offline screenshots (`DSOR_PACKET_REPLAY`), now with bonus challenges,
  amount-bearing recipes and a wielded sword; `DSOR_UI_REPLAY` steps can
  right-click (`"right": true`).
- **Locked recipe books** show how to get them: their text with the unlock
  item named (the book's title for `{0:s:itemname}`) and the origin box
  (`_Template_RecipeBook.LockedIcon`, "Quest reward", "Recipe"...).

Verified: **341 client tests passed** (15 ignored), with tests for every
rebuilt window and for `draw_order`'s inputs; offline captures of the quest
log (quests, achievements overview and rows, dailies, Wanted), the events
(two events, a second bar page), the album (front page, a category, a
preview, a book), the transmog desk (skins, a chosen skin, dyes), the gem desk
and the workbench (tabs, categories, a locked book) from the fixture packets.
Not tried online. Known gaps: the fixture's worn helmet has no converted icon
(its slot shows empty), the replay's right click lands after the inventory
reads it (the desk pick is covered by the unit tests only), scroll handles
cannot be dragged, and the first client run
after a build sometimes holds the loading screen at 75 % of the effect
preload (unrelated to these windows; the next run loads).

### Follow-up: the quest tracker no longer blocks the game bar (2026-10-21)

Once anchored to the right edge, the quest tracker covered the game bar's
buttons above it (Premium, autoplay, fullscreen, the game menu): none could be
hovered or clicked. Its `LabelFull` / `LabelCompact` containers are invisible
(no texture, alpha 0) yet span x 720..1024 from the top of the screen
(y 0..325), and the tracker (depth 46) is drawn over the game bar (30), so
those empty rectangles took the pointer. Invisible artwork now leaves the
pointer to what lies under it in the tracker, as it already did in the chat
windows (`ui::passes_pointer`); the visible frame (from y 71) still takes it.
Test: `the_quest_tracker_leaves_the_game_bar_above_it_to_the_pointer` (fails
without the fix); 103 UI tests passed with the converted assets.

## Interface per-frame cost (2026-10-21)

The systems' windows commit cut the Kingscity benchmark view from about 180 to
95 FPS (50–60 online). It was the interface's per-frame work, not the lighting
merge. The fixed build is back at the pre-regression frame rate (p50 about
4.5 ms against the 10-20 build's 4.7–5.2 ms in interleaved runs). The whole
interface now costs about 0.2 ms a frame. Analysis and numbers:
[native map performance](native-map-performance.md#interface-regression-after-the-systems-windows-2026-10-21);
rules: [docs/ui.md, "Per-frame cost"](ui.md#per-frame-cost).

- `spawn_windows` checks a window's visibility before preparing its layout (it
  prepared every closed menu each frame: 5.95 ms).
- `systems::sync` builds only the open windows' views, when `Net::ui_revision`,
  the inventory, `State`, the catalogs, the class or a shown countdown's minute
  change (1 ms a frame before). `on_command` bumps `ui_revision` for every
  command but the actor stream; local edits call `Net::touch_ui`. `send` writes
  `State` only on a change.
- `quests::sync` rebuilds the quest list on the same signal; `spells::sync`
  hands its model to the windows when it changes; `sheet::rows` skips the
  closed sheet.
- Guards: `ui::UiWork` counters with four regression tests (one checked by
  reintroducing the bug: it fails), a `ui work:` line in `frameprofile` and a
  runner warning, a debug and `uiaudit` cross-check for a missed `touch_ui`, the
  `system-profile` cargo feature (per-system and per-module times, `DSOR_SYSTEM_PROFILE`
  filter), `--env` in `tools/profile_native_maps.py`, and `AGENTS.md`.
- The views' wall clock (events, the Wanted board) no longer uses
  `std::time::SystemTime`, which panics in the browser.

Verified: **346 client tests passed** (15 ignored); with the converted assets
the 13 UI fixture tests pass too (the Kingscity doorway lighting test fails on
the current, un-overridden assets, unrelated to this change). Logs:
`.local/logs/map-fps/kingscity-{regress,sysprof,ab}-*`. Not re-measured online.

## Command ids and layouts of the release261_192 server (2026-10-21)

The live server (`D:\_dso_retired\dso-server\dso_server`, updated 2026-10-21;
the files it replaced are in `dso_server_backup_2026-10-21`) now serves the
release261_192 `dro_client64.exe`: `protocol/command_ids.py` resolves every id
against `docs/command_ids_rel261_192.json` by default, and the combat codecs
write that build's marker bits (`DSO_SKILL_HIT_MARKER_BITS`, on). The client
follows it; every layout below is checked against the server's own encoders
(`tools/prod_vectors.py`, `systems_vectors.py`, `shop_vectors.py`,
`appearance_vectors.py` run against that tree).

- **Ids.** 1–346 are unchanged. That build registers three commands at 347, so
  the 22 later classes move: the Wanted Thief 356–359 → 359–362, the Season
  Pass block +3, ServerToClientMsg 368 → 381 (`commands/names.rs`, now the full
  371-class registry with its actor flags; `MOST_COMMANDS` 382).
- **Skill commands (70–75), server copy:** one inheritance-marker bit between
  SkillBase and the derived fields (`SkillBase::marker`; the client's own skill
  commands go without it, which the server's decoders accept). `Body` has
  `decode_server`/`encode_server` for such direction-specific bodies.
- **Hit (107):** a reserved bit after the damage types (`Hit::tag_boundary`).
- **NewRemotePlayer (33), RemotePlayerInfo (34):** a trailing emblem string;
  the player kind is 4, not 5; the fields once unnamed are the team, the
  honour points and the title type.
- **TimedChallengesInfo (131):** a fourth list, the comeback dailies.
- **Offer (90):** shop offers carry a ninth flag, DirectlyBuyOffer.
- **MatchMakingInfo (215):** the first byte is an event (0 joined, 1 joined by
  the leader, 2/3 left, 4 called off by the server, 5/6 group refused), not a
  state: the PvP window uses `MatchMakingInfo::queued()` (0 or 1); leaving used
  to come as -1 and now comes as 2.
- **MatchInfo (196):** same shape, the server's new labels (kills, deaths,
  damage, score, support, class, rounds lost).
- **New layouts** for what the server now sends: SeasonPassRewardsInfo 354,
  SeasonPassMissionsInfo 358, KickOutToHubLimitTimeLeft 363, KillScore 364,
  EndlessTowerProgress 367, EndlessTowerBuffs 369, ScreenMessage 381 (its text
  goes to the chat). NewNPC batches now start with an ActorsEnterVicinity for
  the new NPCs (decoded already).
- Checked, no client change: NewPlayer (values only), the starter inventory
  (content), the pickup stack merge (an InventoryInfo delta with a replacement
  map, which `InventoryState::apply` already handles), the quickbar layout
  (slots are read by kind), the transport.
- Not done: the server's new client requests (ShuffleTierSpenderDeals 328, the
  Season Pass requests 353/355–357, crafting payment selector 5) have no client
  feature yet. `inventory_release261.json` stays as generated from
  `.local/dso_server`: the live tree lacks its premium-overflow encoder fix.
- **Compatibility:** `.local/dso_server` (patch5 ids, no marker bits) no longer
  matches this client; the live server's patch5 mode (`DSO_COMMAND_IDS=builtin`,
  `DSO_SKILL_HIT_MARKER_BITS=0`) neither.

Verified: **36 proto tests** (the regenerated `release261`, `systems` (58
vectors), `shop` and `appearance` data) and **346 client tests passed** (15
ignored). Not yet run online against the live server.

## Status effects above the action bar (2026-10-21)

The effects on the player now show in `bottombar`'s `BuffBar`: positive ones on
its left half, negative ones on its right half, each with its icon, stack count
and time left, and a hover card. Details: [docs/ui.md, "Status effects above
the action bar"](ui.md#status-effects-above-the-action-bar).

- `tools/status_catalog.py` (run by `export_ui.py` with `bottombar`, or alone)
  writes `interface/status.catalog.json` from the October 2026 client's
  database: 2087 effects (438 negative) marked `BuffUI` / `DebuffUI`, their
  icons (248 new `icons/status_*` PNGs), `_Template_IconConfig` art, names and
  hover lines from `gui.statuseffects.modifier`. 79 marked effects have no icon
  in the client and are left out.
- `client/src/ui/controls/effects.rs` fills the twelve authored slots from
  `Net::stat_effects`: one icon an effect, stacks as the stats count them
  (occurrence + 1, summed, capped at `MaxStackSize`), arrival order kept across
  refreshes, permanent effects giving way past six, time left from the server
  tick in the game's short units. The bar stays click-through.
- Change-driven: the slots are reassigned on `Net::stat_revision`, the labels
  rewritten when the next one is due. `UiWork::status_bars` and the `ui work:`
  line count the assignments.
- `DSOR_TEST_EFFECTS` gives the offline player effects for captures.

Verified: **363 client tests passed** (15 ignored; 6 new), the 14 UI fixture
tests pass with the converted assets. Offline Kingscity captures with
`DSOR_TEST_EFFECTS` and `DSOR_UI_REPLAY` show both halves and the hover cards.
`profile_native_maps.py kingscity` (debug build, with and without ticking
effects): `ui work` 0 in every window after loading, including the status bars.
The `system-profile` build: `effects::draw` 0.003 ms, `effects::sync` and
`effects::fixture` 0.001 ms a frame; the UI module 0.243 ms. Logs:
`.local/logs/map-fps/kingscity-statusbar-*`. Not yet seen online with
server-sent effects.

### Follow-up: the hovered monster's negative effects (2026-10-21)

The hovered monster's panel now shows its negative effects in a row under its
bottom plate, centred on the health bar, with the bottom bar's icons, stacks and
timers ([docs/ui.md](ui.md#on-the-hovered-monster)). No original window has such
a row: `effects::prepare_target` adds six slots and labels to `interactinfo`.
`Net::actor_effects` now keeps the StatusEffect elements of every other actor in
view (stamped per change, dropped on discard and map change). **Settings >
Combat > Status effects on enemies** (default on, `monster_effects`) turns the
row off. `DSOR_TEST_TARGET_EFFECTS` is the offline fixture.

Verified: **365 client tests passed** (15 ignored; new: the monster row's
layout and centring, its change-driven assignment across hover, effect, setting
and panel changes, and the setting's default, row and draft). Offline Kingscity
capture (`DSOR_TEST_MONSTER`, `DSOR_TEST_TARGET=1`): four negative effects under
an Arisen Warrior's panel, its positive one left out. `system-profile` build with
the monster hovered and timers running: `ui work` 0 after loading,
`effects::sync` 0.002 ms and `effects::draw` 0.003 ms a frame, UI module
0.277 ms. Log: `.local/logs/map-fps/kingscity-monsterrow-sysprof.log`. Not yet
seen online.

## The social window, window hotkeys and the game bar's comeback buttons (2026-10-21)

The social window (`socialscreen`) shows what the server says about the players
on our map, our group and our guild. It is a systems window
(`controls/systems/rosters.rs`): a view rebuilt on `Net::ui_revision` and
`State`, the authored row templates cloned. Details:
[docs/ui.md, "Social window"](ui.md#social-window).

- **Bottom background.** `MenuBackground` (the brown panels behind the column
  heads and the bottom controls, a vertex-coloured `system/white` mesh) was
  hidden because it follows the contents in the layout and covered them. It is
  drawn first now, its border line still on top; the invite overlay's panel
  likewise. Every tab, Ignore included, has its bottom panel.
- **Search.** Opening the window sends GroupList, GuildList and PlayerQuery
  kind 1 (the map's players); Enter searches a name (kind 0), Update asks
  again. Rows follow the game's `CreatePlayerListElement` (release261
  0x14052c55c, decompiled in DSOR_Server
  `data/diagnostics/social-online-ui.annotated.txt`): status icon, name, class
  icon, level, group icon.
- **Group and guild** rows from GroupStatus and GuildStatus (leader star,
  location, rank, offline greyed); Leave group, Leave guild and the foundations
  enabled only when they can do something; the guild foundation panel only
  outside a guild.
- **Ignore** shows the server's (always empty) IgnoredStatus; **Invite** is
  disabled. Lists scroll (wheel, their own arrows and handle), the column heads
  sort, a row opens the player menu; tabs and icon buttons have hover titles.
- **Protocol.** PlayerQuery (180) was read as u32, str, u8; the server's
  layout is u8 kind, u32 query id, str text (the old 56-bit capture is a
  kind-1 refresh). New: PlayerQueryResult (181, `Net::player_query`).
  `tools/social_vectors.py` gained three vectors (regenerated against the live
  tree; the 15 existing ones came out byte-identical).
- **Assets.** `export_ui.py` converts the rows' runtime icons
  (`icon_status_online/offline`, `icon_member_warrior/wizard/ranger/dwarf`;
  converted into the local assets with DSO_Godot's converter, no full
  re-export). `struktur_hell.png` is sampled repeating: the rows' UVs run from
  -1 to 3.5, and clamped they smeared into streaks.
- **Hotkeys.** E events, L rankings, J battlegrounds; the bottom-left
  consumable moves from E to G. Bindings revision 2 (`keymap`) migrates saved
  files the same way and leaves a new key unbound where the player already uses
  it.
- **Settings > General:** "Show the Return Link button" and "Show the Return
  Check-in button" (`ExclsiveactiveCodeBtn`, `RecalledSignInBtn`; on by
  default), each button its own page group on the game bar.
- Not done: the Friends tab (BuddyList/BuddyStatus) and the group finder list.
  The live server's PlayerQuery rows carry role 0xFF and "online" as their third
  flag, so the group icon can only mark our own group's members; the server
  would have to fill the row's role for other groups.

Verified: **37 proto tests** (1 new) and **365 client tests passed** (15
ignored; 5 new); with the converted assets 14 of the 15 ignored tests pass (the
Kingscity doorway lighting test fails on the un-overridden assets, as before).
Offline Kingscity captures with the fixture packets (`tools/ui_fixture_packets.py`
now adds a group, a guild and a 22-player search) and a `DSOR_UI_REPLAY` route:
every tab, sorting, scrolling, a row's player menu, the disabled Invite tab,
E/L/J opening their windows, and a scratch settings file hiding both game bar
buttons. `profile_native_maps.py kingscity social-open` (debug build, the social
window open and filled): `ui work` 0 in every window after loading. Logs and
crops: `.local/logs/social/`, `.local/logs/map-fps/kingscity-social-open*`. Not
yet tried online against the live server.
## Pets (2026-10-23)

Players' pets now come out at their side, follow them and leave, as the client
does it (`client/src/actors/pets.rs`, its module comment cites the addresses).

- **Protocol.** Nothing new: the live server only runs the pet's persistent
  status effect (`services/customization.py` `_toggle_switchable_sticker`; the
  local initialization's StatusEffect batch; a remote player's
  `_remote_status_snapshot`). The effect's StartModifiers name the pet
  (`Pet:PetMonster,pet_frogman#~`) and the client makes it from
  `_Template_PetMonster`; the server never sees it. `Net::pet_events` carries
  every StatusEffect element, as `mount_events` does for mounts.
- **Behaviour, from dro_client64.exe** (`Properties::PetProperty`,
  `PetMovementProperty`, `PetSequenceProperty`, decompiled from the
  `.reverse-current` Ghidra project): the pet appears 2.3 behind its master to
  the left (4.3 while riding: `Messaging::Mount`), turned as its master, plays
  its SummonSequence (and PermanentSequence, looping); walks to its master once
  farther than 2.4 (4.4 riding) and stops 1.7 (4.2) from it, at its RunSpeed
  times its master's speed factor; is put back at its place past 20; plays its
  AlternativeIdleSequence after standing 30 s, then every 30-60 s; when its
  effect ends its other sequences stop, it plays its DeathSequence while still
  following (its movement lives on; once that animation is over its own idle
  or run takes over again, the sequencer's idle restart being off for pets)
  and goes after DespawnTime. A master leaving the view or the map takes its
  pet at once, a leaving one too (`PetProperty::OnDeactivate`). A new pet's
  effect arriving before the old one's end (the server's order) swaps them.
- **Phoenixes.** A ReviveCommand whose u32 (cooldown) is not 0 marks the
  revived player's pet (`ActivateAlternativeDespawnSequence`): it then leaves
  with its AlternativeDeathSequence (`pet_phoenix_revive`). The live server
  sends every revive with 0 and has no pet revival, so this only shows with
  such a packet (the fixture below).
- **Not interactive.** Pets carry no Monster, Npc or RemotePlayer: nothing
  hovers, highlights, targets or hits them. They show and hide with their
  master; their sequences' effects use the actor frame as monsters' do.
- **Assets.** `tools/export_pets.py` writes `characters/pets.json` (328 pet
  effects, 178 pets; `convert_assets.sh` runs it after `export_skills.py`). It
  only reads `sequences.json`, so VFX edits are untouched. Ten pets whose
  SummonSequence has no animation get their Spawn clip's length (`spawn_s`).
  Two pets name a skin as their CharacterSet (`pet_pixie_golden_01`,
  `pet_little_bloodmage_01`); the model loader refuses an unknown set, so they
  get the skin list holding that skin (`pet_pixie_golden`,
  `pet_little_bloodmage`). `pet_fire_slime_S8`'s PermanentSequence is not in
  the October client at all (no pack or `sequences.freg` entry): nothing plays.
- `DSOR_TEST_PET=<pet>[,<seconds>]` gives the offline player a pet 3 s after the
  loading screen; `DSOR_SHOT_PET=<s>[:<s>...],<file>` captures at those times.
  `tools/pet_fixture_packets.py` writes a `DSOR_PACKET_REPLAY` fixture from the
  live server's own encoders (sticker status entries, their removal, a remote
  player and its status snapshot, `encode_revive`, DiscardPlayer).
- Following stays an approximation: PathEngine follows the master's agent in
  its library (`PathEngineActorProperty::MoveGotoTarget`); here the pet walks
  straight on the navigation mesh, along `NavMesh::path` where that is blocked,
  re-routed every 0.5 s.
- Noticed, not changed: the client takes NewRemotePlayer's position as world
  coordinates, while the server sends `last_move.relative_position`
  (map-relative) for a player who has moved; such a player (and its pet) would
  first appear off by the map's centre until its next MoveCommand.

Verified: **12 client tests** (placement, thresholds, spawn once the model is
in, following, teleport, dismissal, following while leaving and despawn time,
a swap in one batch, the master leaving with or without a leaving pet, a master
not in yet, hiding with the master, a cooldown revive's alternative despawn);
the whole suite passes (391, 15 ignored). Offline Kingscity captures
(`DSOR_TEST_PET` + `DSOR_SHOT_PET`, debug build): the frogman rising, standing,
its Despawn and butterflies; the dragon's blue spawn, hover, Death and despawn
light. Replay of `pet_fixture_packets.py` through the client's decoder: our
frogman out, following a jump and stopping short, swapped for the dragon in one
packet, a remote player's phoenix following them and playing
`pet_phoenix_revive` after a cooldown revive, gone with its player, our dragon
sent away. `profile_native_maps.py kingscity` back to back in a quiet machine,
with a dragon out vs without: p50 5.58-5.83 ms vs 5.70-6.08 ms, `ui work` 0
after loading. Logs: `.local/logs/map-fps/kingscity-pets-*3/4`. Not yet seen
online against the live server.

## Shadow Souls: players' summons (2026-10-23)

A player's Shadow Soul (`item_mercenary_*`) now comes out beside them, follows
them, fights and leaves as the live server publishes it; other players' souls
too, attackable when their owner is a PvP enemy. The original client's handling
was checked in the July 2026 `dro_client64.exe` decompile (addresses in the
code comments).

- **Server.** The live tree turns the soul's status
  (`minion_mercenary_*`, StartModifiers `ShadowSoul:<monster>`) into an ordinary
  player summon (`services/customization.py` `_activate_mercenary`,
  `services/player_summons.py`): vicinity + NewMonster to every viewer (again,
  unchanged, to a newcomer), monster MoveCommands as it follows its owner (it
  stops 4 m away, runs at monsters within 11 m of the owner), its attacks as
  monster skill commands, its blows as Hits naming it the attacker,
  DiscardMonster when another soul replaces it or its owner leaves. In PvP a
  hostile player's skills can kill it (`services/pvp_combat.py`). Monsters never
  target or damage summons in the live tree: the client draws such hits and
  deaths (the soul's hit and death sequences) but the server sends none.
  Activating one already works: quick slot 9 ("Companion", H) sends
  UseStickerBookItem, the mercenary bag's Use the inventory command.
- **Protocol.** NewMonster's unnamed fields are named from the server's
  `load_player_summon_spawn` and the client's handler: `heading` (the owner's
  yaw), `category` (1 a player's summon, 2 a map companion), `spawn_tick` (0 for
  the map's monsters), `team`. `Faction::stance` is the client's stance test
  (0x14091b3b8, without the world modes and the Damage blocker): our faction
  (NewPlayer's: 0x00010002, type 0) is hostile to the monsters (type 2) and to a
  PvP enemy's soul, neutral to our own soul, peers' souls and map companions
  (type 0). Tower hazards stay enemies, as before. `tools/summon_vectors.py` +
  `crates/proto/tests/summon_release261.rs` check it against the server's bytes
  for the owner, a peer, a PvP enemy, a companion, a monster and a hazard.
- **Not a target.** Monsters our stance is not Hostile to are not hovered (no
  attack cursor, outline, info window or click attack), player bullets fly
  through them, a friendly soul's own bullets (Viridis' arrows) strike monsters
  instead of players, and the local map shows them as friends' dots.
- **Appearing.** A soul turns as its owner faced. Fresh (under 11 ticks after its
  spawn tick, as the original's OnActivate) it plays its SummonSequence (the
  smoke) with its Spawn clip; met later (a newcomer, walking into view) it only
  plays the Spawn clip of its StartAnimationState (`Spawn;Idle`).
- **Leaving.** The original plays a DespawnSequence only on a KillCommand with
  the despawn bit and removes on DiscardMonster; both are done
  (`Kill::despawn`). The live server never sends that kill and discards a
  replaced soul at once, so a summon's discard also plays its despawn
  (`Net::summons`, `monsters::despawn_summons`, a marked DEVIATION): its
  DespawnSequence (the smoke, the body hidden from frame 6), removal after
  DespawnDuration (1.2 s); a dying soul finishes its death.
- **Fighting.** Its blows show as our floating numbers when the attacker's
  summoner is us (the original's Hit handler); souls (`RelaxedIdle`) stand in
  Idle_Hub at rest and their battle Idle for 5 s after a blow, as players.
- **Replay captures.** `DSOR_PACKET_REPLAY` records `{"frame", "capture"}` and
  `{"frame", "quit"}`; `tools/summon_fixture_packets.py` scripts a soul's life
  around the offline player from the server's own builders.
- Found, not changed (map monsters): the original plays a monster's
  StartAnimationState list on every NewMonster (`Spawn;Idle`: 4443 templates,
  3820 without an exported Spawn length, 227 Spawn clips exported as loops) and
  its SpecialSpawnSequence only when fresh, never its SummonSequence; this
  client plays the SummonSequence of any template with a MinionType (most:
  the scaling progression) each time it comes into view, and no start list.

Verified (in an isolated `CARGO_TARGET_DIR`: a session's worktree built into
`target/` and overwrote `dsor-proto`): **391 client tests passed** (15 ignored;
10 new: a fresh vs a met-later soul, despawn timing, a dying soul's discard, a
despawn kill, the summoner's numbers, bullets on both sides, the stance, the
start-state list, NewMonster from the server's bytes) and every proto suite (5
new summon tests). Offline Kingscity captures (`summon_fixture_packets.py`,
debug build): the soul rising beside the player, running after it, striking a
skeleton with our number, a PvP enemy's Ember Knight, our soul's despawn smoke
and removal. Under load (another session compiling) the soul's model can take
over a second to load the first time, after its rise. `profile_native_maps.py
kingscity` with a soul standing by vs none: `ui work` 0 after loading in both
(first build p50 6.1-6.5 vs 5.9-6.2 ms; the final build's timings were swamped
by another session's load). Logs: `.local/logs/map-fps/kingscity-shadowsoul-*`.
Not yet seen online: no character on the live server owns a soul.

## 3D previews: the inventory's figure and the album's sticker (2026-10-23)

The layouts' runtime render targets are drawn (`client/src/ui/preview.rs`,
docs/ui.md "3D previews"):

- **Inventory.** The equipment and trinket pages' `figur` shows the player's
  character, dressed as in the world, playing its `Inventory` state; the
  mercenary page shows the player's Shadow Soul at rest when its
  `minion_mercenary_*` effect runs on the player (the live server puts it there,
  `services/customization.py` `_activate_mercenary`), else the page's empty
  placeholder. The character and the soul are separate stages, so turning the
  pages shows each at once.
- **Collector's album.** The preview box turns the sticker picked, as the
  original does it (decompiled `UI::StickerbookWidget`: the player's own
  character, `Idle_Hub`; a mount shows the player riding it): a costume on the
  player (one more skin part, its mask culls what it covers), an emote the
  player plays again and again, the player on a mount, a pet, a Shadow Soul.
  Decorations (trails, ground markers, auras, wings) only start particle
  effects and keep their icon, as do stickers of a catalog exported without
  looks.
- **Turning.** The areas' DragRotate buttons now turn the subject while the
  left button is held (no click sound, no hover card).
- **Data.** `tools/systems_catalog.py` adds each sticker's `looks`: per item,
  its CharClass, its status effect's wire index, and what that effect shows
  (`SkinModifier`; the cast effect's ride and its `Mounted:Mount`;
  `Pet:PetMonster`; `ShadowSoul`; `StartAnimation`). On the October 2026
  database: 135 costumes, 112 emotes (+6 skins), 238 of 238 mounts, 188 pets
  (+3 community), 5 Shadow Souls; the 107 decorations have none. Re-export the
  systems catalog to get them.
- **Rendering.** Each stage is an image its own camera draws (layers 26 to 28,
  far under the maps, studio light, no map look); it carries `HighlightCamera`
  so every scene-camera query (`With<Camera3d>, Without<HighlightCamera>`)
  leaves it out, and highlight.rs's three own queries leave `PreviewCamera`
  out. A window draws the image as a widget of the texture name
  `preview:<stage>` in its own drawing order; `ui.wgsl` undoes the
  multisampled edges' premultiplied colour. The camera frames a subject from
  its vertices, skinned on the CPU once it has posed (a character's parts carry
  a fixed box, its joints sit off its body under a variation, and bevy's
  skinned bounds grow as joints turn).
- **Fixture.** `DSOR_TEST_EFFECTS=#<wire index>` gives the offline player an
  effect the bar does not show (`#6380:0`: the Ancient Watcher's).

FAILURE (fixed before landing): measured in its bind pose the moment it was
ready, the Blue Dragonspawn (drawn hovering 1.7 to 2.6 above its root) was
framed under its body and culled: an empty box.

Validation: `cargo test -p dsor-client` 396 passed (new: framing, sticker
resolution, the inventory's figures, idle `UiWork::previews`, the preview
areas' pages and DragRotate, the album's turning widget); `--ignored` 14 of 15,
the Kingscity doorway lighting test failing as before. Offline captures (debug
build, Kingscity, `--play 1 0`, UI replays): the character, turned, on the
trinket page and the Ancient Watcher in the inventory; a Shadow Soul, a costume,
an emote, a mount and a pet in the album. `profile_native_maps.py kingscity`:
`ui work` 0 in every window with the previews closed (p50 6.7 ms), and with the
inventory open one preview built while loading then 0 (p50 7.9 ms: the window
and its turning character). Captures, routes and logs: `.local/logs/preview/`,
`.local/logs/map-fps/kingscity-preview-*`. Not yet seen online.

## The currency bar's 2026 display and its currency bag (2026-10-23)

The money in the top left corner was the legacy display (`LabelDefault`: the
Andermant, the coins, honour, draken and glyphs side by side, numbers running
out of their places). It is now the one the 2026 client shows in game,
`LabelIngamePayment`, with its currency bag (`currencybag`);
`client/src/ui/controls/currency.rs`, traced from release261's
`UI::CurrencyBarWidget` and `UI::CurrencyBagWidget` (addresses in the module).

- **Bar.** The Andermant, the gold, silver and copper, and the currency picked
  in the bag (the Badges of Honor by default) with its icon. The plate and the
  dropdown's hover fill, listed after the money, are drawn under it.
- **Bag.** The dropdown and the bag button open it under the bar: the Glyphs
  of Power, then the shops' 18 currencies by `Priority` (Kitchen Coin, Dragon
  Blood Essence, Season Coin, ... Crystals of Truth), each with its icon and
  whole balance. A row picks its currency for the bar and closes the bag; the
  pick is saved at once (`currency` in `game-settings.json`). A press
  elsewhere or Escape closes it. Its view rebuilds with the other systems'
  views (open, and only when the server said something).
- **Fit.** `GetClampedIntString`: the Andermant in 9 digits, the gold in 5,
  the picked currency in 10, then by thousands (`1234567k`, `100M`); a money
  label's font then shrinks until its text fits its rect (`ui::Fit`).
- **Hover texts.** The whole amounts: `1234567890 Andermant`,
  `100000000 Gold 45 Silver 73 Copper`, `15931 Materi Fragment`.
- **Balances.** NewPlayer's +376 (the glyphs) is now read at login.
- **Data.** `tools/systems_catalog.py` adds `currencies` (re-export the
  systems catalog for an older conversion); `tools/ui_fixture_packets.py`
  sends the server's default keyed currencies; `tools/export_ui.py` exports
  the in-game display.

Not wired: the bag button's glow when a currency grows, the payment button.

Validation: `cargo test -p dsor-client` 402 passed (new: clamping, fitting,
the bar's display and order, the bag's rows and frame, idle frames of the
bar's pick); `--ignored` 14 of 15, the Kingscity doorway lighting test failing
as before. Offline captures (debug build, Kingscity, `--play 1 0`, the UI
fixture with the server's large default wallet): the bar, the bag open with a
hovered row, the pick, the hover texts, the glyphs shrunk into the bar.
`profile_native_maps.py kingscity currency-bar`: `ui work` 0 after loading.
Captures, routes and logs: `.local/logs/currency/`. Not yet seen online.

## The creation screen and the roster as the 2026 client has them (2026-10-24)

The compact Bevy dialogs that stood in for character creation and deletion
are gone. The creation screen is now the original `chargenwindow`; deletion
uses the original `confirmdialog`. The roster follows its widget's rules.
The rules come from release261's `UI::CharacterGenerationWidget`,
`UI::CharacterSelectionWidget`, `UI::PremadePackageWidget` and their
managers, decompiled into the live server tree's
`data/diagnostics/chargen-widget.annotated.txt` (140 functions) and
`charselection-widget.annotated.txt`. docs/ui.md, "The character screens",
describes the result.

- **Creation.**
  - Opening: the roster's add button asks for the editor (op 1); the server's
    op 2 opens it, with its initial look.
  - Classes and looks: four class buttons. Gender, ethnicity (the
    `_asian_` / `_black_` body sets), hair and beard, in the tables' `Index`
    order, going round. Each class and gender has hard-coded default hair and
    beard; the Beard row is for men only.
  - The figure plays the class's `CharSelectStart` / `CharSelectIdle` with its
    `CharGenStartSequence` and looping `CharGenIdleSequence` (sounds, the
    mage's ball and light).
  - The name field: the language's letters, 16 at most.
  - Create sends op 3, with no client-side checks. The 13 deny texts follow
    the client's table. Op 5 hands over to the world.
  - Back sends CharacterSelection op 2.
  - Also drawn: the class description list with its slogan, the gendered
    class names, the empty premium start panel, the editor's currency value,
    and the UI sounds.
- **Roster.**
  - Gendered class names, "Level {0:i}", the mentor bonus icons.
  - Add / Buy / disabled-Add by slots and `MaxCharactersPerAccount`.
  - Delete confirms in `confirmdialog` and removes the character at once.
  - The selection deny texts. The "Entering the world..." and "Offline
    preview" notices are gone: the original writes none.
- **Looks.** The world now draws a player's ethnicity, stature, hair and beard
  from NewPlayer / NewRemotePlayer (players were bald before). The selection
  previews take the ethnicity and stature too.
- **Draw order.** Each window of these screens draws its still artwork, then
  its buttons, then its texts. Authored order hid the Create caption under the
  frame's crest and tinted the class description red. (INFERRED; the menu
  windows are unchanged.)
- **Protocol.** `SessionEvent::CharacterEditor` carries the editor's packet;
  `CharacterCreated` reports op 5; `create_character` takes the whole request.
  `tools/chargen_vectors.py` adds 5 vectors from the server's
  `encode_character_generation` (`crates/proto/tests/chargen_release261.rs`).
- **Data.** `tools/export_character_selection.py` adds `creation` to
  `characters/selection.json`: name bounds, most characters, each class's
  outfit and armament, the hair and beard lists, the sequences and 84 English
  texts. Re-export it for an older conversion.
- **Removed.** The login screen's "new character" defaults (name, class,
  gender) and `--new`; the game's own screen replaces them.
- **Fixtures.** `tools/character_selection_preview.py` takes `--editor`,
  `--empty` and `--refuse N`. `DSOR_SELECTION_REPLAY` takes the creation
  screen's steps and `delete` / `confirm`.

Server mismatches, found against the client and left in the server:

- The server's deny reasons 7 ("already generated") and 8 ("unknown") read in
  the client as `badBodyStyle` and `badBodyStature`.
- Its reason 3 shows as `notAlphaNumeric`.
- It accepts names shorter than `CharacterNameMinLength` (3) and any
  non-control characters. The client filters characters while typing but
  sends short names as they are.

Validation:

- `cargo test -p dsor-client -p dsor-proto`: client 419 passed.
  - New tests: the creation rules and request, the deny texts, the back
    button, the slot buttons, the layouts' controls, the description lines.
  - `--ignored`: 14 of 15, the Kingscity doorway lighting test failing as
    before.
- Proto suites all pass, including `chargen_release261` (2).
- Offline captures (debug build, `a0000_char`, the server-encoded fixtures):
  - the four classes, both genders, the ethnicities and a refusal;
  - the roster, the delete confirmation and the roster after a delete;
  - the mage's showcase over time.
- `profile_native_maps.py kingscity chargen-ui`: `ui work` 0 after loading.
- Captures, fixtures and logs: `.local/logs/chargen/`.
- Not yet seen online: no character was created on the live server.

## Bosses' missing body parts, tiny pigeons, the party frames' level rings (2026-10-24)

Neither came from the October assets: the group window's textures, mesh and
layout match the client's sources, and the boss models were identical in the
August conversion.

- **Skin fragments.** A Nebula skin node draws every primitive group its SFRG
  list names, each with a palette of at most 72 joints; DSO_Godot exported only
  group 0. Balor had no right wing, arm or leg (72 of 127 joints weighted);
  Khalys, Sigrismarr, Mhorblik, Sharr Kharab, Nefertari, Mortis Unchained,
  Medusa and others lost parts, and nodes listing only other groups (Mhorblik's
  `skinned1` = [1, 3], Megasargon's `skinned1..3`, many `skinned1` props and
  pets) drew group 0 unskinned, frozen in the bind pose.
  `tools/restore_skin_fragments.py` rebuilds those nodes' meshes, one primitive
  per fragment (39 models, 90 nodes); `convert_assets.sh` runs it first in
  step 5, and it has been applied to the current pack. Group 0 rebuilt by it
  matches DSO_Godot's primitive exactly.
- **Party frames.** The group level's plate stays (GroupStatus has no level),
  so the frame no longer breaks between the header and the first member; each
  row draws its level ring before its texts, so the ring no longer covers the
  level number (only the last row, whose template lists the ring first,
  showed it). The window folds like the quest tracker: its toggle
  (SwitchToCompact / SwitchToFull, `Action::PartyCompact`, kept in
  `Party.compact` across member changes) leaves LabelCompact, the button in its
  frame. The full mode's toggle frame, dropped before, is the right end of the
  header's top edge (its own vertices there are transparent).
- **Whole-model NPCs' bodies.** Server NPCs and level decorations drawn from
  a whole model (not the uniskel humans) never got their character set's
  variation, which monsters do get. Kingshill's 32 pigeons stood a quarter of
  their size (`deco_pigeon_01` scales every joint by 4), and 90 of the 213
  whole-model sets the maps use are off: chickens x1.2, cats x1.3, maggots x1.5,
  gnobs x1.8, goats x0.85, the Atlantean merchants x1.3...
  `monsters::vary_model` (the monsters' step) now also runs in
  `npc::animate_models`, once the model's `sets.json` is in.

Validation: `cargo test -p dsor-client` 426 passed (the party tests now check
the plate and the ring's order). Offline captures: the UI fixture's 3-member
group before/after (release build); Balor, Khalys, Mhorblik, Sigrismarr and
Sharr Kharab (`DSOR_TEST_MONSTER`, map viewer) whole; Kingshill's pigeons by
the stables before/after (map viewer, `--cam 22.5,6.0,-43.5,20.0,3.0,-50.0`).
The varied joints add about 3 000 entities near the pigeons
(`profile_native_maps.py kingscity variation-before / variation-after2
--position 22 1 -46`, release): p50 5.6-6.3 / 5.5-6.1 ms, p95 6.8-7.8 /
7.4-8.8 ms, within the noise of another build running; `ui work` 0.
Not yet seen online.

## The game data from Bigpoint's CDN, and the account screen's strip (2026-10-24)

The client now downloads the original client's data itself, so it no longer
waits for the official launcher's cache. `crates/game-data` (`dsor-game-data`,
also a command) reads the CDN's build key and its 2,844 tables of contents, then
unpacks the 644 NB3 bundles into `game-data/export_win32/` exactly as the tables
list it. The bundle format is simo8902's extractor's (WTFPL), reimplemented in
Rust. Every blob is checked against its md5 from the root key down, and the
database's bundle member `global/export_win32/db/static.db4` lands in
`export_win32/db/`. The account screen runs the download in a thread and shows
it in a strip across the top (stage, bar, bytes, speed, time left, Retry).
*Sign in & play* and *Create account & play* are dimmed and refuse until every
file is on disk; saving the settings is not held up. Details: docs/game-data.md.

- Updates fetch only the files whose md5 changed (single files up to 2,000;
  more unpack the bundles that changed). An interrupted download resumes from
  its manifest, and deleted files come back. Offline, the build installed last
  is checked instead.
- Speed: 16 download connections feed 8 writer threads through a bounded queue.
  Files are written in place with no temporary-file rename (10x faster on the
  HDD volume, see the doc), folders are made up front, and only missing
  members are hashed and unpacked. Every start's check uses one directory
  listing per folder. The connection pool keeps all connections open (ureq's
  default of 10 forced a TLS handshake per table). The screen is held to 60
  frames a second while a download runs.
- `UiWork::game_data_bars` counts the strip's repaints: four a second at most
  while bytes move, none once the run has finished.

Validation: `cargo test -p dsor-game-data` 16 passed (synthetic builds: fresh
folder, check, repair, an update that drops a file, resume, damaged downloads,
offline, a second client). `cargo test -p dsor-client` 421 passed. Real CDN,
build `6a8b704d`: a first install on the HDD volume took 5 min 42 s (2.21 GB,
164,026 files). An install interrupted after 25 s resumed on the NVMe drive with
626 bundles in 3 min 47 s. Every later check takes 1.0 s. The tree has exactly the
tables' 164,026 files (4.75 GB), none outside `export_win32/`. Compared with the
launcher's October 20 tree, 1,484 of 1,500 sampled same-size files are
identical; the files that differ are the October 22 patch's (maps, audio,
languages), and fetching them one by one from the CDN gives the same bytes as
the bundles. Captures of the strip complete, failed and downloading (fast
build). `profile_native_maps.py kingscity game-data-strip` (fast build): `ui
work` 0 when still, 170-207 FPS.

## The account server's static database (2026-10-24)

Signing in now gives the game data the account server's `static.db4` when it
differs from the CDN's copy. The live server plays with the October 20 build's
database plus its admin-buff rows, and status effect wire ids are rowids.
Server (the live tree, `D:\_dso_retired\dso-server\dso_server`; originals in
`dso_server_backup_2026-10-24-static-db`): `services/content.py` publishes the
database the game services load. `GET /api/content` gives its sha256, size and
packed size. `POST /api/content/static-db` with a live session gives the
database packed as the CDN packs files (`__ZN`), at most 6 times per address
per 15 minutes. The file is read between two identical `stat` calls, hashed and
packed once per revision. `/api/status` and the launcher's endpoints are
unchanged; `DSO_PUBLISH_STATIC_DB=0` turns it off. Client:
`dsor_game_data::adopt_static_db` checks size, sha256 and the SQLite header,
replaces `export_win32/db/static.db4` in one step and records it in
`.dsor/overrides.txt`, so the CDN check keeps it (a lost copy comes back from
the CDN and the next sign-in brings the server's again). The account screen runs
it between `/api/login` and connecting; a failure is logged and play goes on.
Details: docs/game-data.md.

Validation: server `pytest` 104 passed (5 new in
`tests/test_account_static_db.py`, plus the account, admin, maintenance and
standalone suites). `cargo test -p dsor-game-data` 20 passed (3 against a
local HTTP account service: install then match, no session or an old server,
a damaged or non-SQLite download; 1 for the override against the CDN check).
`cargo test -p dsor-client` 421 passed. End to end with the live tree's
account service on port 18080 and its real database: the first manifest took
0.6 s (68.8 MB packed to 7.5 MB). A sign-in replaced the CDN's copy
(`4e4b9038...`) with the server's (`81a09c5c...`) in 0.17 s, the next one found
it matching, and a wrong session got 401 with nothing written. The running server
needs a restart to serve the endpoints.

## Items received (2026-10-24)

The notice the original shows under the top navigation when an item is received
was missing: the item's icon and "Daily Login Chest received", for quest
rewards, the daily login chest, chests and packages opened, and event and
spender rewards. It is release261's `UI::MessageWidget` item line
(`messagewindow`'s `ItemMessageLine`), traced in Ghidra:
`HandleMessageCommand` 0x1401C842C, the message manager 0x1405E71C8, type 2 at
0x1405E6E68, item and template texts at 0x1405E53EC and 0x1405E4B74, the
widget's Load and Refresh at 0x1404860A0 and 0x140486D00, and
`InfoTextDuration` at 0x1413765A0. Before this, the client dropped these
MessageCommands (`combat_text` handles types 0x19 and 0x1d only).

- `gameplay::inventory::Receipt` resolves a type-2 message when it arrives
  (`Net::receipts`): the carried item by its id, else the template the server
  adds, or a currency's fixed template.
- `ui/controls/message.rs` queues the notices and shows one at a time for 4 s:
  `lootReceived` or `lootReceivedAmount`, with the item catalog's name and icon
  layers. The window is rebuilt only when the notice changes, and it takes no
  clicks. docs/ui.md, "Items received".
- `tools/misc_vectors.py` adds `item_received`: two receipts from the live
  server's `item_receipt_protocol` (`crates/proto/tests/misc_release261.rs`).

Not done: the window's other lines (info, status, system messages), and the
original's pause while a UI manager flag (+0x329, not identified) is set.

Validation: `cargo test -p dsor-client` 425 passed (new: the receipt's
resolution, the notice's text and icon, the window's widgets, the queue's
timing with no work on idle frames); `--ignored ui::` 14 passed. `cargo test
-p dsor-proto` passed. Offline capture (debug build, Kingscity, `--play 1 0`,
`DSOR_PACKET_REPLAY` with the server's packets: a daily login chest, 3
Splintered Ruby, and 50 Badges of Honor sent as a template): the three notices
in turn under the navigation. `profile_native_maps.py kingscity item-received`:
`ui work` 0 after loading. Captures, fixture and logs:
`.local/logs/item-received/`. Not yet seen online.

## The NPC's quest screen as the 2026 client draws it (2026-10-24)

The quest dialogue used the 2026 layout (`questscreen2`; the 2026 client never
loads the 2018 `questscreen`) but only its speech bubble. It was moved 12
left and 50 down, paged at 780 characters, and had its own captioned buttons
with the icons stripped. The task list and the reward panel were missing.
Rebuilt from release261's `UI::QuestWidget2` (questwidget2.cc: Load
0x1404D219C, Refresh 0x1404D3844, the rewards 0x1404CF838, the buttons
0x1404D1194, the modes 0x1404D57DC) and matched to a 2026 capture of Thabo's
"Full Moon Rising (1/5)" offer. Details: docs/ui.md, "The quest screen".

- `client/src/ui/dialogue/quest.rs`:
  - the bubble, its text wrapped into at least three lines, with highlighted
    words in white and the player's gender and name;
  - icon buttons with tooltips: Back alone while a quest is in progress, Back
    and Accept otherwise;
  - the task list with check marks (staged tasks wait for their prerequisites);
  - the rewards: money template, experience or dynamic experience, item rows.
- The NPC screen sits at its authored place too (it was moved with the
  bubble). The quest screen is drawn under it, so the backdrop no longer
  darkens the portrait.
- Back ends the conversation. Accept and Continue return to the NPC's menu
  when it offers more, else close. Accepting plays `ui_quest_accept`.
- The conversation's view is built only when its inputs change, no longer
  every frame.
- Data: `tools/quest_catalog.py` exports `staged`, each task's `pre`, `part`
  (questNumber of the series' last), `internal`, `dynamic_xp` and reward
  `rarity`. `tools/dialogue_catalog.py` exports `marked` (3,666 texts with
  inline tags) and the `gui.window.questscreen` texts. Both catalogs were
  regenerated into `assets/`; nothing else in them changed.
- The `DSOR_UI_REPLAY` `npc` step takes `npc_quest_done` (the log's finished
  task bits).

Not done: the fades, scrolling a long task list, the quest bonus on
experience, the full-log message, deliveries on this screen. The series total
is inferred (the client's computation was not traced).

Validation: `cargo test -p dsor-client` 432 passed (new: tags and placeholders,
wrapping, the offer's layout, progress and conversations, screens from the
quest data, gendered texts, Unicode wrapping); `--ignored ui::` 14 passed.
Offline captures (debug build, Kingscity, `--npcs --play 1 0`,
`DSOR_FLAGS=allnpcplacements`, Thabo):
- the offer as in the 2026 capture;
- the Accept and Back tooltips;
- Back closing the conversation;
- a quest in progress (Back alone, a ticked task, staged tasks hidden,
  Andermant, coins and XP);
- a completion ("Complete quest").

`profile_native_maps.py kingscity quest-screen`: `ui work` 0 after loading.
Captures and replays: `.local/logs/quest-screen/`. Not yet seen online.

## Map state as the server sets it: its maps, scenery events, exits (2026-10-25)

Testing the Windows release (docs/release.md) on the live server, Kingshill
showed NPCs and props the server never meant to: the Halloween, Christmas and
anniversary NPCs among them.

- **The cause:** the CDN's October 22 build gave every row of its 391 map
  databases a new `Guid`. The server and the October 20 client (the dev
  `assets/`) still share the old ones; the release's conversion of the CDN build
  shared none (0 of Kingshill's 119 NPCs).
- **Why it showed:** the live server sends every NPC of a map, those of
  events not running too, and leaves hiding them to the client
  (`player_init_world.py`, `include_inactive_event_sets=True`). The client finds
  an NPC's event rule in `npcs.json` by Guid; with no match nothing hid the 68
  NPCs of events not running, and none faced its way. Server props did not
  bind either.
- **The fix:** the account service publishes its map databases as it does
  `static.db4` (dso_server `services/content.py` `MapDatabasesPublisher`,
  `POST /api/content/maps`, live tree backup `dso_server_backup_2026-10-25-maps`).
  The client installs them at sign-in (`dsor_game_data::adopt_server_data`,
  `.dsor/server-maps.txt`) and a release refreshes every table made from
  `static.db4` or `maps/*.db4`, the stamp naming the bundle
  (docs/game-data.md, "The account server's databases").

The client's own map state, which also showed what the server never sent:

- **Scenery events** (`world/map.rs`): the manifest's `event_rules` (base
  placements gone while a flag runs: 458 in Kingshill) and `event_instances`
  (the levels' `.eve` layers: 2,740 in Kingshill, among them the fairground the
  server unlocks for good) were never read. Base placements now go and event
  placements come with the server's flags; a change rebuilds only the cells
  whose placements changed. The ids are level-data's (`placement_ids`; all 458
  Kingshill rules match). The browser build keeps the base scenery.
- **Exits** (`world/exits.rs`): ExitInfo (68), decoded but unused, now says
  which exits stand: an always-present one active or inactive (not usable), a
  map event's only while active. `exits.json` carries each exit's Guid (both
  exporters); older sidecars match by name.
- **Event state** (`world/props.rs` `NetworkProps`): a running event's tags gate
  content as its name does (dso_server `active_event_flags`), and it ends at its
  end time, which the server never sends; the revision moves then, so what it
  gated goes.

Validation: `cargo test -p dsor-client` 439 passed (new: scenery conditions,
event tags and expiry, ExitInfo, which exits stand); `-p dsor-game-data` 26 (new:
map bundles installed, kept, fetched again, refused); `-p dsor-level-data`
`--include-ignored` (every Kingshill rule names a placement); dso_server
`test_account_map_dbs.py` 5 + static db tests 12. Against the live tree's account
service (another registry): the release's game data took the 391 maps in 1.8 s,
then `--database-only` (5.0 min) left Kingshill's NPCs 119/119, exits 21/21 and
props 13/13 on the server's Guids. `profile_native_maps.py kingscity`: 140-160
FPS (the build before: 114-130 in the same warm run), `ui work` 0 after loading.
Not yet seen online: the live server was down after a crash.

## The Skills & Talents window: Experience, Wisdom, Group, Honor (2026-10-25)

Requested with four reference shots of a dwarf's window; the
English texts come from the localization. The four tabs are drawn and work as
release261 draws them (docs/ui.md, "The Skills & Talents window"). Before this
change only the skill icons were drawn: no ranks, no level, no points, and the
other three tabs were empty.

- **Spec.** `Talents::SkillTalentsWidget`, `ClientTalentManager` and
  `PlayerTalentModule` were decompiled into the live server tree's
  `data/diagnostics/skilltalents-widget.annotated.txt` and
  `talents-manager.annotated.txt`. The rules the server enforces are in
  `services/talents.py`. The four shots match the database: every tree's points
  are spent (Experience 100 with Covering Fire's 7 ranks from equipment,
  Wisdom 2865, Group 2025, Honor 43).
- **Data.** `tools/talent_catalog.py` writes `interface/talents.catalog.json`
  (and `export_ui.py` writes it with the window). The catalog asset and its
  rules (costs, levels, points, tokens) are in `gameplay/talents.rs`.
- **Protocol.** TalentSelectionInfo now decodes contexts 1, 2 and 4, and Net
  merges a one-tree context into the full snapshot. A skill-preset refresh
  (context 3) used to replace the whole snapshot and lose Wisdom, Group and
  Honor. TalentCategoryPointsInfo keeps its four values whatever its category
  byte. UnlockSkillTalentPreselectionInfo (285) is kept, and
  UnlockSkillTalentPreselection (284) was added.
- **Window.** `controls/systems/talents.rs` adds the view, the layout fill, the
  reset confirmation (`confirmingamepaymentdialog`) and the split dialog's
  talent mode. Requests go through `systems::send`. The old `Action::Talent`
  and preset tabs are gone.
- **Renderer.** Widgets' `Saturation` is exported and drawn. Artwork in another
  texture tile is wrapped, which brings back 207 icons, among them the reset
  gears, the social window's icons and the transmog desk's reset buttons.
- **Layout export.** Talent cells are placed as the original places them (4 px
  higher). The Group art and the Honor boxes are drawn behind their content;
  before, they covered the Group level bar and the Honor talents.
- **Other.** The HUD's wisdom bar shows the Wisdom level and progress.
  Equipment talent ranks (`gameplay::stats::TalentBonuses`) appear in green. A
  trader can open without an NPC (`Dialogue::trade`: the Wisdom Shop, the PvP
  Merchant).

Validation: `cargo test -p dsor-client` 454 passed, `-p dsor-proto` all passed.
The new tests cover:

- the view against the server's picks and the catalog's rules;
- the client-side refusals, right clicks, resets and unlocks;
- a preset cooldown;
- no rebuild on idle frames or on unrelated effects;
- costs, levels and tokens;
- equipment bonus ranks.

With `--ignored`, `the_real_window_fills_its_tabs_from_the_view` passes
against the converted assets. So do the other UI asset tests, except the
known Kingscity doorway lighting test.

Captures were made with `tools/talent_fixture_packets.py`
(`DSOR_PACKET_REPLAY`, `--play 3 0`). They show all four tabs, the
talent/focus/skill cards, the reset confirmation (2074 Gold for 100 points at
level 100) and the split dialog.

`profile_native_maps.py kingscity` with the window open: 160-170 FPS, `ui work`
0 after the first build. The same with `--flags uiaudit` logged no errors.
Closed: 175-184 FPS (the difference is the window's drawing).

The server's gaps found here were closed the same day (see "The talent rules
on the server" below).

Not seen online yet.

## Buying from a trader with a right click (2026-10-25)

The trading screen now takes orders as the 2026 client does. Before, a left
click anywhere on a card bought the offer, and a right click did nothing.

Decompiled in the live server tree's Ghidra project:

- `TradingWidget::HandleEvent` (0x14055FB8C): a right click on an item `Icon`
  calls `BuyItemClick`.
- `TradingPort::HandleMessage` (0x140560848): once the order is queued, it
  plays `ui/trader/ui_trader_buy`, then `ui/general/ui_click`.
- `ClientTradingManager::SellItem` (0x14027F3E8): the same with
  `ui_trader_sell`.
- `TradingWidget::UpdateMouseCursor` (0x140565F3C): `mousecursor/buy` over a
  trader's icon, `mousecursor/sell` over an inventory icon.

What changed:

- **Target.** Each card's `ItemSlot/Icon` is the control (`NpcOffer:<choice>`,
  `controls::Action::Offer`).
- **Clicks.** A right click buys one copy of the offer, or buys the stack back.
  A left click orders nothing, and the rest of the card takes no clicks.
- **Sounds.** The buy sounds play when the order is sent (online, one order at
  a time). Right-click selling plays the sell sounds.
- **Cursor.** `cursor::TradeHover` shows the buy cursor over a trader's item.
  It shows the sell cursor over a carried item that a right click would sell
  (`controls::items::sell_cursor`).

Added the same day:

- **Unaffordable offers.** An offer the wallet does not pay is greyed (its item
  at saturation 0) and its icon takes no click, so no order is sent. The check
  follows the server's debit (`commerce_protocol.debit_purchase_currency`),
  currency by currency, after the veteran discount. The shop's own currency
  comes from the new `currency_items` of `tools/trading_catalog.py` (the
  current `dialogue.catalog.json` was patched with the same query).
  `currency::Wallet` gained the legacy balance (`price_uc`).
- **Large Andermant purchases.** From 20,000 Andermant
  (`_Globals.LargeRCExpensesNotificationThreshold`), `controls::consent` asks
  first, as the original's `AskForLargeRCExpenseConsent` does (0x1403E9E6C). It
  uses `confirmdialog`'s `DefaultDoNotShowAgain` layer; the dialog's page
  `confirm_layer` now picks between it and the item-destroy layer. Ticking "Do
  not show this window again" and confirming adds the original's DialogId 6 to
  the new `Preferences::skipped_dialogs`.
- **Replay fixtures.** `wallet` and `npc_offers_rc` (offline only).

Not reproduced:

- dragging an item icon into the bags;
- `IsOfferPurchasable`'s messages (which currency is missing), and its free bag
  slot check;
- greying buy-back cards (their price is not in the snapshot).

Tests: 465 client tests passed, 7 of them new (the card wiring, the
right-click-only order, the trade cursors, the wallet rules, the greyed card,
and two for the consent). With `--ignored`, the UI asset tests pass except the
known Kingscity doorway lighting test.

Offline replays of the gem merchant used `npc_offers`, because the old
`.local/merchant-offers.bin` no longer decodes ("invalid offer category: 92").
The first replay:

- a left click on an icon logged no order;
- right clicks on two icons logged choices 3 and 4;
- a right click on a card's text did nothing.

The second replay had 50,000 Andermant and offers at 25,000 and 60,000:

- the 60,000 offer was greyed and its right click did nothing;
- the 25,000 offer asked first, and Cancel dropped it;
- ticking the box and confirming saved `skipped_dialogs: [6]` (in a scratch
  working directory), and the next right click did not ask.

`profile_native_maps.py kingscity` (debug build): `ui work` 0 after the first
build. Not tried online yet.

## The talent rules on the server, and the preset countdown (2026-10-25)

Requested after the Skills & Talents window: "add the server support for the
missing features ... and wire the implemented features to the client". The
live server tree (`D:\_dso_retired\dso-server\dso_server`) now applies the
rules the window applies. The originals of the changed files are in
`dso_server_backup_2026-10-25-talents` (new file:
`tests/test_talent_services.py`).

Rules added (`services/talents.py`, `protocol/talent_protocol.py`):

- **Unlock levels.** A Wisdom or Group talent's `UnlockLevel` is compared with
  the character's level, not with that tree's level.
- **Exclusive groups.** A skill talent is refused while another talent of its
  `GroupId` has ranks in the preset.
- **DeselectTalent (280).** It takes a focus point back or switches a trait off,
  and replaces the talent's effects.
- **UnlockSkillTalentPreselection (284).** It buys the next Skill preset for
  `UnlockPreselectionCost` Andermant (10000, 30000, 50000, 100000), then sends
  CurrencyChanged and 285. The count is saved (`AccountState`
  `unlocked_skill_presets`) and sent at login; before, 285 always said 0.
- **Preset limit.** A switch past the membership's `NumTalentSets` (none 2,
  Premium 3, Deluxe 4), or to a Skill preset not bought, is refused.
- **Preset cooldown.** Outside a hub (ruleset 5), a switch puts the tree's
  `SkillPresetCooldown` / `KnowledgePresetCooldown` / `GroupPresetCooldown`
  (300 s) on the player as a saved, timed effect, and refuses other switches
  while it runs. The client keeps an effect until the server removes it, so a
  timer sends the removal when it ends. Entering a hub lifts it ("or you must
  enter an urban area"), and a map load re-arms the timer.
- **Reset prices.** ResetTalents (286) and ResetTalentCategory (287) cost gold,
  priced as the client prices them. The factor comes from
  `talent_reset_costs_buff`; Honor is free. A reset the wallet cannot pay is
  refused.
- **Wisdom Shop.** `skilltree_wisdom_trader`, opened without an NPC, needs
  `trait_wisdom_all_dungeon_shop`.
- **Classes.** `niwalk` traits have a class.

A refused command is answered with the server's state: TalentCategoryPointsInfo
and TalentSelectionInfo, or 285.

Client: the waiting presets' cards now count the cooldown down from the
effect's end tick ((end - now) x 0.04 s, `talents::card`). Before, they named
the full 300 s.

Validation:

- `tests/test_talent_services.py`: 14 passed (codecs, preset rules, prices; and
  through a full login: deselect, unlock levels, groups, purchase and relog,
  membership, cooldown start, refusal, expiry, re-arm and hub lift, both
  resets, the trader gate).
- dso_server suite: 1348 passed, 211 failed. The same 211 fail on a copy with
  the original files (1334 passed there): combat, summon, wave and mount-speed
  tests that were already failing.
- `cargo test -p dsor-client`: 457 passed. The cooldown test now checks the
  countdown. With `--ignored`, `the_real_window_fills_its_tabs_from_the_view`
  passes.

The live server needs a restart to load this. Not tried online yet. Still
missing on both sides: `TalentRespecBlock`.

## The loading screen no longer holds at 98% in Kingshill (2026-10-25)

Online in Kingshill, the loading screen stayed at 98% with every readiness
check done. Its log said "render warm-up, progress 100%", and the HUD said
`models 488/487`.

- **The cause:** the server's events take away base scenery (Kingshill's
  `event_rules`). When a model's last placement went, the events path
  (`world/map.rs`, since "Map scenery, exits and events follow the server")
  recounted the wanted models but kept the model loaded. `CurrentMap::progress`
  counted the loaded models over all handles against the wanted count, and the
  loading screen waits for `done == total`: 488 never equals 487. `main.rs`'s
  `total - done` would underflow on the same numbers.
- **The fix:** `progress` counts only wanted models, and the events path
  releases a model whose last placement went, as streaming releases those of
  cells left behind. Its log line now says how many it released.

Validation: a new test (`progress_counts_only_the_models_still_wanted`);
`cargo test -p dsor-client` 458 passed. Offline Kingshill with the EventUpdate
and ExitInfo the live server sends at login (taken from its own init code):
"map events changed 6 cells, released 2 models", HUD `models 535/535`. The old
code would have kept those 2. Not yet tried online.

The loading screen also tells players it has not frozen (`screens/loading.rs`).
A status line under the caption names the step in plain words and shows the
seconds taken. A light crosses the bar while work goes on. After 15 s with
neither the percent nor the step moving, a note says the game is still working.
After 45 s, **Enter anyway** (once the map and the player are in) and **Close
client** appear. Tests: stall timing, wording and buttons (3 new, 461 passed).
A temporary build held the screen to capture each state: the status line, the
note, then both buttons. A normal offline Kingshill load still finishes with no
stall warning.

## Faster client builds (2026-11-05)

After a change or a new commit, `Play.cmd`'s release build spent 12–14 minutes
on the client crate alone. Type checking and the rest of the front end take
38 s of that (`-Z time-passes`). The remainder was LLVM optimising the crate as
one codegen unit on a single thread, then thin LTO re-optimising every
dependency with it.

Client crate rebuilt on its own (dependencies already built), and the frame
time in the Kingscity fixed view (`tools/profile_native_maps.py`, median p50
of rounds 2–5; round 1 was slower for every build):

| Build | Client crate | Frame p50 |
|---|---|---|
| release, 1 codegen unit (before) | 741–867 s | 4.73 ms |
| release, 16 codegen units | 281 s | 4.78 ms |
| release, 256 codegen units | 312 s | |
| release, incremental, one-line edit | 171 s | |
| `fast` profile, one-line edit | 24 s (14 s warm) | 5.09 ms |

- **Release:** the client crate builds in 16 codegen units
  (`[profile.release.package.dsor-client]`); its frame time is unchanged within
  noise. Dependencies stay in one unit, so they were not rebuilt. Incremental
  compilation does not help release builds, because thin LTO goes over every
  dependency again each time.
- **Play.cmd:** `tools/Build-Client.ps1` builds the `fast` profile and
  `Play.cmd` starts `target/fast/dsor-client.exe`. Frames are about 7% slower
  than release (~195 against ~210 FPS in that view). Releases
  (`tools/release/package.*`), `profile_native_maps.py` and the level editor
  still use `target/release`, so it and Play.cmd no longer force each other's
  client rebuild. `[profile.fast]` inherits release's package overrides, so it
  sets the client back to 256 units (with 16, every incremental rebuild redid
  bigger units).

Validation: `cargo build --release -p dsor-client` after the change took
4 m 22 s (rustc ran with `-C codegen-units=16 -C lto=thin`); that executable
ran the Kingscity view at p50 4.65–4.75 ms. `tools/Build-Client.ps1` took 22 s
after a touched source file and 2.6 s with nothing changed; its executable
loads Kingscity and renders. Every run logged the missing
`skills/emotes.json`, which predates this change.
