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
| Prelight | light buffer: global light, point lights | bevy forward lighting: the level's global light, its back light, ambient and placed lights (crate::lighting) |
| Color | Solid, AlphaTest, DecalReceive | opaque / alpha-mask surfaces; cull mode from the n3 |
| ColorAlphaDecals | ground decals, SequenceAlpha | crate::decals (projected on the CPU once) |
| ColorAlpha | AlphaLit, AlphaUnlit, PreAdditive, ParticleLit, PostAlphaUnlit, Alpha, Additive, in that batch order | bevy's transparent pass, sorted back to front whatever the batch (gap: no batch order) |
| BrightPass, Bloom x4 | bright pass of the half-res scene, blurred at quarter res | bevy `Bloom` (gap: no BloomColor tint) |
| HighlightSilhouette, GatherOutline | outline of the hovered / targeted actor | **missing** |
| HiddenCharacterColor | characters behind walls as a silhouette | **missing** |
| Refraction | refraction surfaces, over everything drawn | crate::refraction over bevy's copy of the opaque scene (gap: transparent surfaces drawn after it are not refracted) |
| pe_compose | colour x 2 + bloom x BloomScale, saturation, balance, gamma, highlight, vignette x VignetteColor, fade | no tonemapping (the default; the F2 panel can turn TonyMcMapface on), bloom, `ColorGrading` saturation, bevy `Vignette` (exact for VignetteSize 2) |

## The level's look (`_Instance_AmbienceBubble`, `_Instance_Light`)

`tools/export_ambience.py` writes `maps/<map>.ambience.json`; crate::lighting
applies it, converting Nebula's gamma-space light units to bevy's (`UNIT`, the
inverse of the camera's exposure: what Nebula shows as `v`, bevy shows as `v`):

- global light: `sat(N.L) x LightColor x LightIntensity` from the bubble's z axis,
  plus `LightOppositeColor x LightIntensity` from the other side (a second
  directional light, approximating `sat(BackLightFactor - N.L)`), plus
  `LightAmbient` (bevy's ambient divided by its 0.452 diffuse BRDF factor);
- fog: linear between `FogNearDist` and `FogFarDist` (25 and 50 in Kingshill: the
  game camera sits 25 units from the player, so the fog starts at the player),
  `FogColor`, opacity `FogIntensity` (UNVERIFIED mapping);
- bloom (threshold `BrightPassThreshold / 2` in gamma, intensity x `BloomScale`),
  saturation, vignette;
- the light that follows the player (`LocalLightColor x LocalLightIntensity` at
  `PlayerLightOffset`; UNVERIFIED range);
- the other bubbles (714 permanent ones in 149 levels: caves, interiors): inside
  one (box or sphere through its transform), the highest priority's look fades in
  over its `PEFadeTime`;
- placed lights (15 069 always-on rows): point lights with Nebula's linear falloff
  matched at half range, flickering ones wandering (UNVERIFIED curve), each in its
  culling cell so they go out with it.

Not yet: height fog (`HeightFogColor`, `FogNearHeight` / `FogFarHeight`), bloom tint,
`Balance`, the characters' rim light (`RimLightColor`, `RimLightIntensity`),
`LightShadowIntensity`, `LightSpecularIntensity`, projected spot lights
(`LightType` 1, 329 rows: they need `LightTexture` and shadows), event bubbles
(`EventSetRow` other than -1).

## Surfaces, per 2018 shader

Counts are models of the 2018 data using the shader.

| Shader | Models | 2018 (disassembly) | Here |
|---|---|---|---|
| standard | 10 723 | lit diffuse + EmsvMap0 x MatEmissiveIntensity + spec; Intensity0 (`mayaAnimableAlpha`) x alpha | StandardMaterial; emission at the node's intensity; render state (Solid / AlphaTest / Additive / unlit) and **cull mode** from the n3; static Intensity0 applied |
| particle | 4 181 | Alpha: tex x vcol x (1 + MatEmissiveIntensity), alpha too; Additive: tex x vcol, no emission; both x sat(depth behind / (Intensity0 + 0.05)); sprite phases (AlphaRef, Intensity1 fps); stretch along the second position | crate::particles on the CPU, drawn by `NebulaMaterial` kind Particle: both formulas, **soft edges** (depth pre-pass), **stretch**, **sprite phases**, additive faded by the fog |
| environment | 1 941 | lit + cube(reflect(eye, N)) x Amplitude x SpecMap0.r | `NebulaMaterial` Environment: **cube reflection** (cube maps from `tools/embed_textures.py`); the spec mask is 1 - roughness (the conversion kept only a roughness map) |
| decal | 864 | projected decal, Intensity0 x alpha | crate::decals; static Intensity0 applied |
| refraction | 757 | scene behind moved by DuDv x Intensity1 x **10** px; alpha = AlphaBlendFactor x vcol.a; DuDv scrolled by Velocity | crate::refraction, **x 10 fixed** (was 10 times too weak), alpha fixed (was Intensity0); **map refraction surfaces are drawn** (the flat map path skipped them) |
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
hit frame. Missing:

- **HitCommand (115) is not handled** (crates/proto decodes it; client/src/net.rs
  ignores it): no hit sequence on the victim (`_Template_Monster` / `_Template_NPC`
  `PhysicalHitSequence`, `FireHitSequence`... `CriticalHitSequence`,
  `HeavyHitSequence`), no HitColor flash (`customColor2` in every lit shader), no
  bullet impact sequence (134 skills: `bullet.impact`), and bullets go through
  their targets to the end of their lifetime;
- KillCommand: death sequences (`DeathSequence`, `DespawnSequence`);
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

## Bug dumps

F9 (or Ctrl+Shift+D) downloads `dsor-dump-<time>.json` (natively: writes it in the
working directory); it is also written by itself on a panic or when bevy quits on
a rendering error. It holds the build commit, the URL (session id hidden), the
browser and WebGL renderer, the game state of the last second (map, camera,
player, network, FPS, look settings, flags) and the last 600 log lines
(client/src/dump.rs).
