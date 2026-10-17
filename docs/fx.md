# Effects and shaders: the 2018 client and this one

What the 2018 client (rel 206.7) draws, read from its own data, and how this
client draws it. Every claim about the 2018 client comes from its files:

- `export_win32/frame/Default.xml`: the frame shader (passes, batches, node filters);
- `export_win32/shaders/shaders_sm30`: the compiled D3DX effects, disassembled with
  `tools/shader_dis.py` (techniques, render states, SM3 bytecode with constant
  names, preshaders);
- the `.n3` models: each surface's shader, render state (node filter), textures and
  parameters; the parameter a shader sees is the effect parameter whose
  **semantic** is the n3 variable (`tools/shader_dis.py params <pool> <effect>`:
  `displacementFactor <Intensity1>`, `Reflectivity <Amplitude>`...);
- the level databases (`maps/<map>.db4`, SQLite): `_Instance_AmbienceBubble`,
  `_Instance_Light`, `_Instance_GlobalShadow`.

## The frame

| 2018 pass (Default.xml) | What it does | Here |
|---|---|---|
| NormalDepth | normal + depth of Solid / AlphaTest surfaces | bevy depth pre-pass (`DepthPrepass`): natively always, in the browser with `?flag=depth` (WebGL2 binds it only without MSAA) |
| CollectShadows, BlurShadows | the global light's shadow, blurred | bevy cascaded shadow map, one cascade |
| Prelight | light buffer: global light, point lights | bevy forward lighting: the level's global light, its back light, ambient and placed lights (crate::rendering::lighting) |
| Color | Solid, AlphaTest, DecalReceive | opaque / alpha-mask surfaces; cull mode from the n3 |
| ColorAlphaDecals | ground decals, SequenceAlpha | crate::rendering::decals (projected on the CPU once) |
| ColorAlpha | AlphaLit, AlphaUnlit, PreAdditive, ParticleLit, PostAlphaUnlit, Alpha, Additive, in that batch order | bevy's transparent pass, sorted back to front whatever the batch (gap: no batch order) |
| BrightPass, Bloom x4 | bright pass of the half-res opaque scene, blurred at quarter res | `nebula_bloom`, including the bubble's `BloomColor`, threshold and scale |
| HighlightSilhouette, GatherOutline | outline of the hovered / targeted actor | **missing** |
| HiddenCharacterColor | characters behind walls as a silhouette | **missing** |
| Refraction | refraction surfaces, over everything drawn | crate::rendering::refraction over bevy's copy of the opaque scene (gap: transparent surfaces drawn after it are not refracted) |
| pe_compose | colour x 2 + bloom x BloomScale, saturation, balance, gamma, highlight, vignette x VignetteColor, fade | bubble-driven bloom, saturation, RGB balance and vignette in one display-space pass before FXAA; neutral brightness/gamma and no tonemapping |

## The level's look (`_Instance_AmbienceBubble`, `_Instance_Light`)

`tools/export_ambience.py` writes `maps/<map>.ambience.json`; crate::rendering::lighting
applies it, converting Nebula's gamma-space light units to bevy's (`UNIT`, the
inverse of the camera's exposure: what Nebula shows as `v`, bevy shows as `v`):

- global light: `sat(N.L) x LightColor x LightIntensity` from the bubble's z axis,
  plus `LightOppositeColor x LightIntensity` from the other side (a second
  directional light, approximating `sat(BackLightFactor - N.L)`), plus
  `LightAmbient` (bevy's ambient divided by its 0.452 diffuse BRDF factor);
- fog: linear between `FogNearDist` and `FogFarDist` (25 and 50 in Kingshill: the
  game camera sits 25 units from the player, so the fog starts at the player),
  `FogColor`, opacity `FogIntensity` (UNVERIFIED mapping);
- bloom (threshold `BrightPassThreshold / 2` in gamma, intensity x `BloomScale`,
  tint `BloomColor`), saturation, RGB `Balance`, and vignette colour/intensity/size;
- the light that follows the player (`LocalLightColor x LocalLightIntensity` at
  `PlayerLightOffset`; UNVERIFIED range);
- the other bubbles (714 permanent ones in 149 levels: caves, interiors): inside
  one (box or sphere through its transform), the highest priority's look fades in
  over its `PEFadeTime`;
- placed lights (15 069 always-on rows): point lights with Nebula's linear falloff
  matched at half range, flickering ones wandering (UNVERIFIED curve), each in its
  culling cell so they go out with it.

Camera colour is controlled by the active ambience bubble in both the game and
editor camera monitor. Saturation and balance are applied after bloom, followed
by the authored vignette; zero bloom still applies the other effects. All these
values fade with `PEFadeTime` when entering or leaving a local zone. The editor's
explicit selected-bubble preview uses the same composition. Older assets without
`Balance` use neutral `[1, 1, 1, 1]`.

F2 and `look.json`/browser preferences now adjust scene lighting and fog only.
Legacy saved `saturation`, `exposure`, `bloom`, `vignette` and `tonemapping` values
are ignored, so they cannot override the authored camera colours. Death/revival
temporarily desaturates and dims the view without changing the bubble settings.

Height fog follows distance fog on fogged materials, as in the 2018/261 shaders:
none above `FogNearHeight`, all of `HeightFogColor` past `FogFarHeight`, linear
in world height between, mixed in gamma space like their `lrp`. It fades with the
other bubble values.

The characters' rim light (`shd:character` and `characterfalloff` only, not
monsters) adds `RimLightColor x RimLightIntensity x pow(max(dot(n_view, dir), 0), 4)`
to their lit colour in gamma space, before the hit colour and the fogs. The
engine's direction is fixed, (4, 4, 1) normalised in view space (from the upper
right), read from the running release261 client. `DSOR_FLAGS=norim` leaves it out.

Not yet: `LightSpecularIntensity`, projected spot lights
(`LightType` 1, 329 rows: they need `LightTexture` and shadows), event bubbles
(`EventSetRow` other than -1).

Bubble membership uses the local player's feet after movement transforms have
propagated. The map viewer uses the camera's physical position. Disabled volumes
remain disabled through asset export.

Ambience transitions cross-fade the linear lighting contributions of the source
and target presets over `PEFadeTime`. Each direction stays fixed; changing a look
never sweeps the sun across the scene. An interrupted transition starts from the
visible mixture, including its outgoing directions. Repeated crossings merge
matching directions, and a completed fade retains only the target preset.

The sun entity supplies one shadow map, fixed to the exported `_Instance_GlobalShadow`
direction (falling back to the default ambience direction in older assets).
`LightShadowIntensity` controls the shadowed share of each preset's direct light.
Local presets additionally scale that share by their linear direct-light luminance
relative to the map default (capped at 1). This preserves outdoor shadows while
weakening their contrast in dim interiors, even when ambient light also falls.
Zero direct light leaves no directional shadow contribution.
Back light remains unshadowed. Native and browser builds share the same blend;
it fits WebGL's single-directional-light limit. Studio/thumbnail cameras retain
their own lighting.

The renderer adapter in `client/src/rendering/lighting/blend.rs` reuses Bevy's PBR
BRDF for each contribution and binds a small view uniform in the unused probe
bind group. The client uses material cube maps rather than view environment
probes; adding view probes or clustered decals requires extending this binding
integration. Shader adapters check their Bevy source anchors when loading.

Kingscity's two permanent interior volumes have authored entrance corrections in
`level-overrides/a0200_kingscity.json`: the church begins at its open doors
(`x = 140`, beyond the porch at `x = 137`), and the palace begins at the room-side
entrance caps (`z = -143`, beyond the arch at `z = -140`). Only the entrance face
of each box moves; its other bounds and lighting settings are retained. Reapply
these overrides after conversion with `tools/apply_level_overrides.py`.

## Keeping the player visible indoors

`client/src/rendering/occlusion.rs` casts five rays from the active game camera to the local
player's torso, head, lower body and sides. A reveal starts only when all five
body samples are blocked by opaque or alpha-tested map geometry. Low furniture
and narrow obstacles stay intact when any sample remains visible. Once fully
blocked, the geometry receives a local, soft-edged dithered opening. All
blocking layers are considered, including walls behind other walls and separate
meshes that jointly obscure the body. This also works for outdoor map geometry
when it obscures the character; it does not infer indoors from brightness or
ambience priority.

The opening follows the player, fades in over 0.16 seconds, holds for 0.12 seconds
across brief gaps, then closes over 0.25 seconds when any body sample clears.
Geometry at ground level and behind the character stays intact. Characters,
particles and transparent effects
are excluded. Original mesh geometry remains available to raycasts, so opening
the hole does not make the blocker oscillate between visible and hidden.

Cutout materials are temporary per-entity variants of the original standard or
Nebula material. Shared materials, merged mesh batches and collision/navigation
data are preserved, and the original material handle returns when the obstruction
clears or the player disappears. The depth/normal prepass matches the opening;
shadow cameras still draw the complete building.

For native offline verification, `DSOR_OCCLUSION_REPLAY=<steps.json>` accepts steps
with `frame`, optional `position: [x,y,z]`, `capture: "path.png"`, and `quit: true`.
Frames start after the map has spawned. The route only runs in offline play.

## Surfaces, per 2018 shader

Counts are models of the 2018 data using the shader.

| Shader | Models | 2018 (disassembly) | Here |
|---|---|---|---|
| standard | 10 723 | lit diffuse + EmsvMap0 x MatEmissiveIntensity + spec; Intensity0 (`mayaAnimableAlpha`) x alpha | StandardMaterial; emission at the node's intensity; render state (Solid / AlphaTest / Additive / unlit) and **cull mode** from the n3; static Intensity0 applied |
| particle | 4 181 | Alpha: tex x vcol x (1 + MatEmissiveIntensity), alpha too; Additive: tex x vcol, no emission; both x sat(depth behind / (Intensity0 + 0.05)); sprite phases (AlphaRef, Intensity1 fps); stretch along the second position | crate::rendering::particles on the CPU, drawn by `NebulaMaterial` kind Particle: both formulas, **soft edges** (depth pre-pass), **stretch**, **sprite phases**, additive faded by the fog |
| environment | 1 941 | lit + cube(reflect(eye, N)) x Amplitude x SpecMap0.r | `NebulaMaterial` Environment: **cube reflection** (cube maps from `tools/embed_textures.py`); the spec mask is 1 - roughness (the conversion kept only a roughness map) |
| decal | 864 | projected decal, Intensity0 x alpha | crate::rendering::decals; static Intensity0 applied |
| refraction | 757 | scene behind moved by DuDv x Intensity1 x **10** px; alpha = AlphaBlendFactor x vcol.a; DuDv scrolled by Velocity | crate::rendering::refraction, **x 10 fixed** (was 10 times too weak), alpha fixed (was Intensity0); **map refraction surfaces are drawn** (the flat map path skipped them) |
| uvanimated | 737 | uv + Velocity x t; alpha x Intensity0 | UV keys (pos / scale / rot) on effect models; **Velocity scrolls in the shader** (`NebulaMaterial` Scroll) so merged map surfaces (waterfalls) move; static Intensity0 applied |
| water | 578 | two scrolling normal maps (Intensity0 / Scale tiling, Intensity3 speed, Intensity2 second speed), cube reflection x Intensity1, lit colour x Amplitude, alpha max(reflection, Amplitude) x tex.a x vcol.a x soft border (BumpScale) | `NebulaMaterial` Water, all of it (soft border with the depth pre-pass) |
| simplelayer | 427 | lerp(DiffMap0, DiffMap2 (uv1 x Intensity1), DiffMap3(uv1).r), same for spec / bump | `NebulaMaterial` Layer: **the second colour layer** (DSO_Godot dropped DiffMap2/3); not its normal (BumpMap1) and spec (SpecMap1) |
| uvanimated2 | 405 | uv through TextureTransform rows | UV animator keys; Velocity scroll as uvanimated |
| unlit | 360 | DiffMap0, fog only | unlit; static Intensity0 applied |
| volumefog | 302 | tex x vcol x (1 + emissive); alpha sat(max(vcol.a, tex.a) x depth behind x Intensity0) x silhouette (Intensity2) x Intensity1; scroll by Velocity | `NebulaMaterial` VolumeFog: **drawn** (was hidden): window light shafts, black depth fog in arches |
| monster, character, characterfalloff, monsterexp | 380 | lit x 1.5 + 0.2625 x colour, item dyes (MatDiffuse / MatSpecular masked by SpecMap0 g / b), overlay DiffMap1, cube reflection, rim light, HitColor flash | **StandardMaterial only**: next step for characters |
| glow | 108 | no texture: additive shell, N.V through FresnelPower's curve x MatDiffuse x Amplitude, pushed out by FresnelBias x 0.1 | `NebulaMaterial` Glow (were **solid white shells**); not the push-out, not an animated Amplitude |
| ocean, coast | 46 | water plus vertex waves (Velocity, Vector0..2) and foam | drawn as Water, without the waves and foam |
| sequenceadditive / sequencealpha, unlitalphavertexcolors, skybox, lightmapped2, treetrunk | 18 | sprite-sheet sequences, test shaders | StandardMaterial |

Plants: the 2018 vegetation is shd:standard, AlphaTest, with no wind (no
vegetation shader reads the time). What was wrong here is that every converted
material was double-sided: leaves modelled with a reversed copy of each face drew
both copies over each other. `tools/embed_animators.py` now writes each material's
`doubleSided` from the n3 `CullMode` (2 = back faces culled: 32 605 materials),
which also halves their rasterisation.

Glass and windows: lit windows are shd:standard with an emissive map (their colour
now that the scene is not tonemapped and the level's light is the data's), glass
panes shd:environment (cube reflection), light shafts shd:volumefog.

## Skills and combat

Drawn: the sequencer's effect models, attached graphics, point lights, camera
shakes, bullets flying straight and their death sequence, skill impacts at the
hit frame, blows (HitCommand 115) and deaths (KillCommand 116) on monsters.

A bullet (`client/src/gameplay/skills.rs` fly_bullets) meets the actors in its
way: its BulletRadius against their CapsuleRadius, swept over each frame's
flight, flat as the server tests it (any height within 4 units of the capsule,
so a monster downhill is met too). It passes through ImpactCount of them, each
with its BulletImpactSequence, and ends with the skill's impact on the last one,
placed on that actor's ground. The server's blow (HitCommand / KillCommand naming
the attacker) ends the attacker's bullet on its victim even when the client's own
test would have missed it (a monster that died meanwhile, one standing a little
off). Aimed at a monster, a bullet dips or climbs to that monster's chest. Shifted
skills whose SkillBulletId is a bullet row (stunshot) are drawn on the aimed
point, like the other Shifted skills. Missing:

- bullet motions other than Straight (Homing, LockOnHoming, Orbiting...);
- Pre / Post sequences (3 skills, dwarf turrets);
- shader animators of Amplitude / Fresnel* (glows that pulse), animators on
  NebulaMaterial surfaces.

## Cost and the browser

Kingshill from the game camera, before / after this work (same converted data):

| | before | after |
|---|---|---|
| native, M1 Pro, 2560 x 1440 (CPU ms, HUD) | 6.3 | 9.7 (7.3 with depth pre-pass, bloom and level lights off) |
| Chrome / ANGLE Metal, shadows off (frame ms) | ~23 | ~32 |
| Chrome / ANGLE Metal, shadows on | ~1 FPS | ~1 FPS |
| Chrome, **WebGPU** build, everything on (shadows, decals, bloom, lights, depth pre-pass) | - | 60 FPS (vsync), 5 ms CPU; Wildforest the same |

Found in the browser (headless Chrome on a Mac, `docs` reproduction: serve
`client/dist`, open `?map=a0200_kingscity`):

- **The sun's shadow pass is the browser's bottleneck on Kingshill**, in the
  original build too: ~1 FPS with shadows, 30-40 without, the CPU at 4-5 ms (the
  GPU waits ~1 s a frame for Metal). Not the filter (Hardware2x2 changed nothing),
  not the resolution (512 changed nothing), not alpha-tested foliage; the tutorial
  map's shadows cost nothing. To find: per-draw cost in ANGLE's Metal path.
- WebGL2 takes one directional light: the back light is native only.
- Kingshill lost its WebGL context ("GL_OUT_OF_MEMORY ... Failed to allocate host
  memory") a few seconds after the ground decals were projected (349 boxes,
  293 426 triangles), in the original build too; the dump (F9, or by itself on a
  render error) shows the sequence.
- The browser client is built twice (`tools/build_web.sh`): WebGPU and WebGL2
  (client features `webgpu` / `webgl2`), and `client/web/index.html` loads the
  WebGPU one when `navigator.gpu` gives an adapter, the WebGL2 one otherwise
  (`?gpu=webgl2` / `?gpu=webgpu` force one). WebGPU lifts what WebGL2 forbids here
  (GPU preprocessing and clustering, several directional lights, the depth
  texture with MSAA) and does not have the shadow pass problem above.

Quality switches: `?flag=depth` (browser: depth pre-pass for the soft edges),
`?flag=nobloom`, `?flag=nolights` (no placed lights), `?flag=noshadows`,
`?flag=lowshadows` (512 shadow map), `?flag=noparticles`, `?flag=nodecals`;
natively `DSOR_FLAGS=nodepth,...`.

## Rendering census

`DSOR_FLAGS=renderaudit` (browser: `?flag=renderaudit`) logs a scene census every
five seconds after loading. Natively, set `DSOR_RENDER_AUDIT=<path.json>` to save
the latest complete report, including component counts, scene hierarchies and
meshes bypassing frustum culling. The census is opt-in because walking the whole
world adds diagnostic overhead; disable it for performance measurements.

The HUD's **mesh candidates (all views)** counts `Mesh3d` entities whose
`ViewVisibility` is true. This includes shadow views and is evaluated before GPU
culling. It is not a draw-call count or a count of visible pixels. The census
separately reports the game camera's bounding-volume/frustum candidates, light
view candidates, hidden map cells, skinned meshes and animation targets.
Main-camera candidates can still be behind other geometry or partly off screen;
triangle totals omit meshes whose CPU geometry has been discarded. Total
entities also include bones, scene nodes, UI and other objects with no 3D mesh.

### Kingshill investigation (2026-10-15)

Native RTX 4070/Vulkan, 1280 x 720, offline camera at
`143.109375,18.67601,-38.910706` looking at
`155.609375,0.9983387,-51.410706`. The offline `--npcs` mode includes all placed
NPCs as well as map decorations, so its population differs from a server session.
Captured after all 603 map models loaded and 650 settling frames:

| Configuration | Total entities | Loaded 3D meshes | All-view candidates | Main-camera candidates | Unique shadow candidates |
|---|---:|---:|---:|---:|---:|
| NPCs, decorations, shadows | 67,012 | 6,152 | 989 | 403 | 916 |
| Same camera, shadows disabled | 67,012 | 6,152 | 391 | 391 | 0 |
| Map only, shadows enabled | 14,128 | 3,145 | 558 | 187 | 498 |

Separate runs sample different animation/particle states; these are scene counts,
not performance benchmarks. Shadow candidates overlap main-camera candidates and
must not be added to them. The baseline's 586 candidates outside the main view
are accounted for by light views. Off-screen objects can cast on-screen shadows.

- 65 of 82 map cells were hidden. Of 6,152 loaded meshes, 5,019 were hidden by
  hierarchy visibility; all ordinary map and NPC meshes had bounds and enabled
  frustum culling. The 30 explicit culling bypasses were particle batches.
- NPC/decorative-character hierarchies accounted for 52,529 entities, map
  hierarchies for 6,907. The world contained 20,350 animation targets and 14,059
  extra scale nodes used by body variations. These nodes are not mesh draws.
  Only 3,436 targets retained `AnimatedBy` in the sampled frame.
- All 324 placed NPC/decorative actors were resident, including 204 dressed
  characters. Distance culling hides far actors and parks animations, but keeps
  their entities. The UI additionally retained 5,687 `Mesh2d` entities.
- Static map geometry was merged into 2,037 meshes across 24-metre cells.
  A partly visible merged mesh submits its whole geometry. Main-view map
  candidates contained about 519k CPU-readable triangles; this does not measure
  rasterized pixels or surviving triangles after GPU clipping.

Follow-up targets: actor residency/streaming and animation work for actors outside
all useful views; tighter shadow-caster/batch bounds; particle batch visibility;
and measured GPU occlusion culling for geometry behind opaque objects. Preserve
off-screen casters that affect the visible scene. No culling policy was changed
as part of this census. Local evidence: `.local/logs/render-audit/` (JSON reports,
logs and screenshots for `baseline`, `no-shadows`, and `map-only`).

### NPC animation visibility

NPCs and decorative characters using `AnimLod` now park their bone targets after
their meshes have been absent from **all camera and shadow views** for 250 ms.
Their animation clocks, state changes and cross-fades continue. The next Update
after a visible result restores their targets, without restarting the clip or
waiting for the next animation-LOD frame. Distance-hidden actors still stop as
before, but distance checks now happen before the LOD early return.

This uses the completed CPU visibility pass, not GPU occlusion results. Actors
behind walls can therefore still animate if they intersect a view's frustum.
Local players, portraits, characters still loading their meshes, and actors with
particle emitters retain their previous behavior. The particle exception keeps
animated attachments correct when their particles reach beyond the body's bounds.
Scene/outfit hierarchy changes invalidate cached mesh ownership; unrelated UI
hierarchy changes do not. Entity-keyed lookup tables keep the per-frame scan cheap.

The same code runs natively, on WebGPU, and on WebGL2; it has no vendor-specific
dependency. For an A/B comparison, disable only this new gate with
`DSOR_FLAGS=noanimviewcull` or `?flag=noanimviewcull`. Keep `renderaudit` disabled
when timing. This optimization reduces animation work, not retained entity counts
or mesh draw candidates.

Same-executable native Kingshill A/B captures at the investigation camera:

| Configuration | Animating characters, disabled → enabled | Attached bone targets, disabled → enabled |
|---|---:|---:|
| Shadows enabled | 41 → 38 | 3,436 → 3,230 |
| Shadows disabled | 41 → 22 | 3,436 → 1,813 |

Both versions retained 67,278 entities in this asset snapshot. Clip/particle
sampling can vary between runs; these are workload counts, not FPS gains. A
concurrent compiler affected the census runs, so their timing is not a benchmark.
Evidence: `.local/logs/anim-visibility/final-{shadow,no-shadows}-{before,after}.{json,png,stderr.log}`. The native
test suite passed 85 tests (12 existing asset-dependent tests remained ignored),
including visibility grace/reentry, shadow-only visibility, outfit reparenting,
particle attachments and preserved clip phase.

Short native A/B timing runs with the census disabled did not establish a
reproducible overall FPS gain. One shadow-enabled repeat also had a different
all-view candidate count, so it is not an equivalent rendering workload. Do not
translate the target-count reduction into an FPS percentage. Browser performance
has not been measured for this change.

## Bug dumps

F9 (or Ctrl+Shift+D) downloads `dsor-dump-<time>.json` (natively: writes it in the
working directory); it is also written by itself on a panic or when bevy quits on
a rendering error. It holds the build commit, the URL (session id hidden), the
browser and WebGL renderer, the game state of the last second (map, camera,
player, network, FPS, look settings, flags) and the last 600 log lines
(client/src/diagnostics/dump.rs).
