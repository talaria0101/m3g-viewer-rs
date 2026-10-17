# m3g-viewer (Rust, macroquad)

A viewer for M3G (JSR-184, the mobile 3D graphics format). It is a port of the
Java/LWJGL `m3g_viewer` (extracted from KEmulator) to Rust on
[macroquad](https://github.com/not-fl3/macroquad). The goal is the same pixels
for every frame, including animation, not a similar-looking viewer.

## Build and run

```sh
cargo build --release
./target/release/m3g-viewer assets/walls.m3g
```

It needs an OpenGL 2.1 (or newer, compatibility or not) context with GLSL 100
support, which is what miniquad asks for. There is no fixed-function GL
dependency: unlike the Java viewer it does not need a compatibility profile.

| Input | Action |
| --- | --- |
| Left-drag | Orbit |
| Scroll | Zoom |
| `W` `A` `S` `D` | Pan (one step per key press) |
| `R` | Re-frame the model |
| `Esc` | Quit |

| Option | Effect |
| --- | --- |
| `--headless` | Batch mode: no interaction, render and write a PNG |
| `--frames N` | Render `N` frames at a fixed 16 ms step, then stop (implies batch mode) |
| `--out FILE.png` | Write the last frame to `FILE.png` (default `shot.png`) |
| `--dump-dir DIR` | Also write every batch frame to `DIR/frame_NNNN.png` |
| `--dump-depth FILE` | Write the last frame's depth buffer (raw little-endian `f32`, bottom row first) |
| `--scene-camera` | Use the camera stored in the file instead of auto-framing |
| `--tex-filter nearest\|linear` | Override the texture filter the file requests |
| `--disable-light-clamp` | Do not clamp lit vertex colours before interpolation |
| `--force-perspective-correction` | Accepted; has no effect (see below) |
| `--quit-after N` | Interactive mode: exit after `N` frames (smoke tests) |
| `--selfcheck` | Load the shipped assets from `./assets`, check animation moves, exit |

Batch mode opens a normal (visible) 800x600 window: macroquad cannot create a
hidden one. On a machine without a display use a virtual one
(`Xvfb :99 -screen 0 1280x1024x24 +extension GLX`).

External texture references in an `.m3g` (the shipped assets use them) resolve
against the directory of the file that names them, so keep the PNGs beside the
models. `http:` references are rejected.

## What "the same render" means here

The Java viewer draws with the OpenGL fixed-function pipeline. macroquad
(miniquad) exposes only shaders, so the port reproduces the fixed-function
behaviour explicitly:

* **CPU, per vertex, in the order GL applies them:** model-view (including the
  position bias/scale GL multiplies in last), projection, clip position, fog
  coordinate, lighting (GL 2.1 model: ambient/diffuse/specular, spot and
  attenuation, two-sided, local viewer, vertex-colour tracking, the colour clamp),
  texture matrices, flat-shading provoking vertex, triangle-strip winding.
* **GPU, per fragment (`src/gfx.rs`):** texture environment (replace, modulate,
  decal, blend, add, for all five image formats, up to four units), fog, alpha
  test, two-sided colour selection. Blending, culling, depth test/write, colour
  mask and polygon offset are pipeline state.

Matrix products and dot products use the same evaluation order as the GL
implementation they were measured against (`math::dp4` is a fused multiply-add
chain). That detail is not cosmetic: with the plain `a*b + c*d + ...` order the
depth buffer differed by 1 to 2 least-significant bits on 24% of a mesh's pixels
and flipped depth ties between coplanar meshes.

### Evidence

All numbers below are from one machine and one GL implementation (Mesa 26.2.5
llvmpipe, LLVM 22.1.8, 256-bit, under Xvfb), because the Java original and this
port must run on the same GL to be comparable. The Java original is
`tools/java-ref` (patched only to dump frames). `tools/compare.sh` renders both
and diffs every frame pixel by pixel.

| Check | Result |
| --- | --- |
| `walls`, `snake`, `obstacles`, default camera, 400 frames each (16 ms step) | 1200 of 1200 frames bit-identical (debug build); 120 frames each also identical in the release build |
| same, `--scene-camera` (parallel projection), 60 frames each | 180 of 180 identical |
| same, `--tex-filter linear`, 60 frames each | 180 of 180 identical |
| same, `--disable-light-clamp`, 10 frames each | 30 of 30 identical |
| depth buffer of the shipped meshes, compared as raw floats | identical |
| scene state (transforms, morph weights, alpha, visibility; every interpolation mode, 6 times per scene) vs Java, as raw float bits | `cargo test` (`tests/golden.rs`) passes |
| 51 generated scenes in `tests/data` (lights, fog, flat/two-sided, blending, alpha test, all texture formats and functions, wrap, filter, texture transforms, two texture units, vertex colours, node alpha, visibility, scope, animation kinds, morph, skinning, parallel camera), 20 frames each | 46 bit-identical, 5 within one 8-bit level (below) |

The five generated scenes that are not bit-identical all use lighting:
`animations`, `lights`, `lights_local_viewer`, `morph_normals_colors`,
`vcol3_lit_track`. They differ from the Java render in at most 51 pixels per
480,000, by one 8-bit level. The GL implementation normalises eye-space normals
with a reciprocal square root that llvmpipe approximates, which this port cannot
reproduce from outside. On a hardware GPU neither side's low bits are
predictable; expect the same kind of difference there. Lit scenes where the
colour saturates (every shipped asset) are unaffected.

### Known differences and limits

* **Not drawn:** `Sprite3D` nodes and `Background` images. The original draws
  both with `glDrawPixels` and pixel zoom, which has no equivalent in a shader
  pipeline. The viewer prints one warning and skips them; a background's colour
  is used. The shipped assets contain neither.
* **Texture units** beyond the fourth are ignored (warning).
* **Skinned meshes** are ported and the generated `skinned` scene matches the
  Java render, but no shipped asset uses them.
* `--force-perspective-correction` is accepted for compatibility. The original
  sets `GL_PERSPECTIVE_CORRECTION_HINT`, which Mesa ignores; this port has no
  such hint at all.
* A corrupt zlib section is an error here; the original logs it and continues
  with zero-filled data.
* Trigonometry uses the platform `libm` where the Java viewer uses `StrictMath`
  or JIT intrinsics. All shipped assets and generated scenes agree bit for bit.
* Per-object hardware differences (different GPU, driver, rasteriser) are
  outside what a port can control.

## Layout

```
src/scene.rs      object graph (arena of M3G objects), node transforms, track rules
src/loader.rs     .m3g parser, PNG references, section checksums
src/anim.rs       keyframe sequences (all interpolations) and the animation pass
src/geometry.rs   morphing and skinning of vertex data, with the original's quantisation
src/pipe.rs       scene flattening, sort order, effective alpha
src/lighting.rs   GL 2.1 vertex lighting
src/gfx.rs        shaders, pipelines, textures, draw call (via macroquad's miniquad context)
src/render.rs     per-draw state: matrices, lighting, texture units, triangle expansion
src/camera.rs     orbit camera and scene bounds
src/main.rs       command line, macroquad window, batch and interactive loops
src/bin/imgdiff.rs  pixel diff of PNG frames (used by tools/compare.sh)
tools/m3gen.py    writes the generated test scenes
tools/compare.sh  Java vs Rust frame comparison
tools/java-ref/   patch that turns the original viewer into a frame-dumping reference
tests/            golden state test, loader robustness tests, generated scenes
```

`gfx.rs` goes through macroquad's miniquad context (`get_internal_gl`) rather
than `draw_mesh`, because macroquad's mesh path cannot carry per-vertex front
and back colours, a clip-space position, or three texture-coordinate components.
Window, input, main loop, screen capture and texture handling are macroquad's.
miniquad ties the depth test to depth writes and has no polygon offset, so those
states are set with a few raw GL calls after each pipeline is applied
(`gfx.rs`, `Gfx::draw`).

## Verifying

```sh
cargo test                          # golden state vs Java, loader robustness
./target/debug/m3g-viewer --selfcheck
tools/compare.sh assets 120         # needs the patched Java build, see tools/env.sh
tools/compare.sh scenes 20
```
