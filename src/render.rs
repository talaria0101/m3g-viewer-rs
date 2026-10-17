//! The renderer: turns scene objects into draw calls (a port of `Renderer`).
//!
//! The original drove the OpenGL fixed-function pipeline. Here the parts of
//! that pipeline which are per-vertex (modelview, lighting, texture matrices)
//! are evaluated on the CPU in the same order the GL matrix stack would apply
//! them, and the per-fragment parts (texture environment, fog, alpha test) run
//! in `gfx`'s shader.

use std::collections::HashMap;

use miniquad::{CullFace, FrontFaceOrder, RenderingBackend};

use crate::geometry::{self, ResolvedVb};
use crate::gfx::*;
use crate::lighting::{Light, Lighting, MaterialParams};
use crate::math::{self, Mat4};
use crate::pipe::RenderPipe;
use crate::scene::*;

pub struct Config {
    pub force_perspective_correction: bool,
    pub disable_light_clamp: bool,
    /// `FILTER_NEAREST` / `FILTER_LINEAR` to override the file, or `None`.
    pub tex_filter: Option<i32>,
}

/// The camera the renderer reads its projection and scope from.
pub enum CameraSource {
    Scene(Id),
    Synth(CameraData),
}

pub struct Renderer {
    pub gfx: Gfx,
    pipe: RenderPipe,
    camera: CameraSource,
    /// World-to-camera.
    view: Mat4,
    lights: Vec<(Id, Mat4)>,
    cfg: Config,
    warned: Vec<&'static str>,
}

/// GL's `glTranslatef` on a matrix `m` (row-major view): `m = m * T`.
fn gl_translate(m: &mut Mat4, x: f32, y: f32, z: f32) {
    let a = &mut m.m;
    for r in 0..4 {
        a[r * 4 + 3] = a[r * 4] * x + a[r * 4 + 1] * y + a[r * 4 + 2] * z + a[r * 4 + 3];
    }
}

/// GL's `glScalef`: scales the first three columns.
fn gl_scale(m: &mut Mat4, x: f32, y: f32, z: f32) {
    let a = &mut m.m;
    for r in 0..4 {
        a[r * 4] *= x;
        a[r * 4 + 1] *= y;
        a[r * 4 + 2] *= z;
    }
}

impl Renderer {
    pub fn new(ctx: &mut dyn RenderingBackend, cfg: Config) -> Renderer {
        Renderer {
            gfx: Gfx::new(ctx),
            pipe: RenderPipe::new(),
            camera: CameraSource::Synth(CameraData::new(NodeBase::default())),
            view: Mat4::IDENTITY,
            lights: Vec::new(),
            cfg,
            warned: Vec::new(),
        }
    }

    fn warn_once(&mut self, key: &'static str, message: &str) {
        if !self.warned.contains(&key) {
            self.warned.push(key);
            eprintln!("[m3g-viewer] WARN {message}");
        }
    }

    /// `Renderer.setCamera`: `transform` is the camera's own transform; the view is its inverse.
    pub fn set_camera(&mut self, camera: CameraSource, transform: Option<Mat4>) {
        self.view = match transform {
            Some(mut t) => {
                t.invert();
                t
            }
            None => Mat4::IDENTITY,
        };
        self.camera = camera;
    }

    /// `collectWorldLights`.
    pub fn collect_world_lights(&mut self, scene: &Scene, world: Option<Id>) {
        self.lights.clear();
        if let Some(w) = world {
            self.collect_lights(scene, w, w);
        }
    }

    fn collect_lights(&mut self, scene: &Scene, world: Id, group: Id) {
        let Some(g) = scene.group(group) else { return };
        for &child in &g.children {
            if matches!(scene.kind(child), Kind::Light(_)) {
                if let Some(t) = scene.get_transform_to(child, world) {
                    self.lights.push((child, t));
                    continue;
                }
            }
            if matches!(scene.kind(child), Kind::Group(_)) {
                self.collect_lights(scene, world, child);
            }
        }
    }

    /// `clearBackground`: starts the frame. Background images are not drawn.
    pub fn clear_background(&mut self, ctx: &mut dyn RenderingBackend, scene: &Scene, world: Option<Id>) {
        let background = world.and_then(|w| match scene.kind(w) {
            Kind::World(wd) => wd.background,
            _ => None,
        });
        let mut color = 0;
        if let Some(b) = background {
            if let Kind::Background(bg) = scene.kind(b) {
                color = bg.color;
                if bg.image.is_some() && bg.crop[2] > 0 && bg.crop[3] > 0 {
                    self.warn_once(
                        "background-image",
                        "Background images are not drawn (the original uses glDrawPixels); only the colour is.",
                    );
                }
            }
        }
        self.gfx.begin(
            ctx,
            (
                math::float_color(color, 16),
                math::float_color(color, 8),
                math::float_color(color, 0),
                math::float_color(color, 24),
            ),
        );
    }

    /// Ends the frame started by `clear_background`.
    pub fn finish_frame(&mut self, ctx: &mut dyn RenderingBackend) {
        self.gfx.end(ctx);
    }

    fn projection(&self, scene: &Scene) -> Result<Mat4> {
        match &self.camera {
            CameraSource::Scene(id) => match scene.kind(*id) {
                Kind::Camera(c) => c.projection_matrix(),
                _ => Err("active camera is not a Camera".into()),
            },
            CameraSource::Synth(c) => c.projection_matrix(),
        }
    }

    fn camera_scope(&self, scene: &Scene) -> i32 {
        match &self.camera {
            CameraSource::Scene(id) => scene.node_base(*id).map(|n| n.scope).unwrap_or(-1),
            CameraSource::Synth(c) => c.node.scope,
        }
    }

    /// `render(node, null)`: draws `root` and everything below it.
    pub fn render(&mut self, ctx: &mut dyn RenderingBackend, scene: &Scene, root: Id) -> Result<()> {
        if !matches!(
            scene.kind(root),
            Kind::Group(_) | Kind::World(_) | Kind::Mesh(_) | Kind::MorphingMesh(_) | Kind::SkinnedMesh(_) | Kind::Sprite3D(_)
        ) {
            return Err("render: node must be a Sprite3D, Mesh or Group".into());
        }
        self.pipe.push_render_node(scene, root, Mat4::IDENTITY);
        self.pipe.sort();

        let mut resolved: HashMap<Id, ResolvedVb> = HashMap::new();
        let mut result = Ok(());
        let count = self.pipe.list.len();
        for i in 0..count {
            let (node, submesh, trans, alpha) = {
                let ro = &self.pipe.list[i];
                (ro.node, ro.submesh, ro.trans, ro.alpha_factor)
            };
            if let Kind::Sprite3D(_) = scene.kind(node) {
                self.warn_once(
                    "sprite",
                    "Sprite3D nodes are not drawn (the original uses glDrawPixels); skipping them.",
                );
                continue;
            }
            let mesh = scene.mesh(node).expect("non-sprite render objects are meshes");
            let (Some(indices), Some(appearance)) = (mesh.submeshes.get(submesh).copied(), mesh.appearances[submesh]) else {
                continue;
            };
            if !resolved.contains_key(&node) {
                match geometry::resolve(scene, node) {
                    Ok(vb) => {
                        resolved.insert(node, vb);
                    }
                    Err(e) => {
                        result = Err(e);
                        break;
                    }
                }
            }
            let vb = &resolved[&node];
            let scope = mesh.node.scope;
            if let Err(e) = self.render_vertex(ctx, scene, vb, indices, appearance, &trans, scope, alpha) {
                result = Err(e);
                break;
            }
        }
        self.pipe.clear();
        result
    }

    #[allow(clippy::too_many_arguments)]
    fn render_vertex(
        &mut self,
        ctx: &mut dyn RenderingBackend,
        scene: &Scene,
        vb: &ResolvedVb,
        indices: Id,
        appearance: Id,
        model: &Mat4,
        scope: i32,
        alpha_factor: f32,
    ) -> Result<()> {
        if self.camera_scope(scene) & scope == 0 {
            return Ok(());
        }
        let ap = match scene.kind(appearance) {
            Kind::Appearance(a) => a,
            _ => unreachable!("appearances are Appearance objects"),
        };
        let strips = match scene.kind(indices) {
            Kind::TriangleStripArray(t) => t,
            _ => return Err("submesh is not a TriangleStripArray".into()),
        };

        // Appearance defaults, as `new PolygonMode()` / `new CompositingMode()` would give.
        let pm = match ap.polygon_mode.map(|p| scene.kind(p)) {
            Some(Kind::PolygonMode(p)) => p.clone(),
            _ => PolygonModeData::default(),
        };
        let cm = match ap.compositing_mode.map(|c| scene.kind(c)) {
            Some(Kind::CompositingMode(c)) => c.clone(),
            _ => CompositingModeData::default(),
        };
        let material = match ap.material.map(|m| scene.kind(m)) {
            Some(Kind::Material(m)) => Some(m),
            _ => None,
        };
        let fog = match ap.fog.map(|f| scene.kind(f)) {
            Some(Kind::Fog(f)) => Some(f),
            _ => None,
        };
        // Only the combinations the original's GL calls could express are reachable here.
        let _ = self.cfg.force_perspective_correction; // GL_PERSPECTIVE_CORRECTION_HINT has no effect on modern drivers.

        let projection = self.projection(scene)?;

        // Modelview: view * model, then the position bias and scale GL applies last.
        let mut mv = self.view.mul(model);
        let sb = vb.pos_scale_bias;
        gl_translate(&mut mv, sb[1], sb[2], sb[3]);
        gl_scale(&mut mv, sb[0], sb[0], sb[0]);
        let mvp = projection.mul(&mv);

        // Lighting state.
        let lighting = material.map(|m| {
            let mut lights = Vec::new();
            for (id, light_to_world) in &self.lights {
                if lights.len() >= 8 {
                    break;
                }
                let l = match scene.kind(*id) {
                    Kind::Light(l) => l,
                    _ => continue,
                };
                if l.node.scope & scope == 0 || !self.pipe.is_visible(scene, *id) {
                    continue;
                }
                let view_light = self.view.mul(light_to_world);
                lights.push(Light::from_m3g(l, &view_light));
            }
            let model_ambient = if self.lights.is_empty() { [1.0, 1.0, 1.0, 1.0] } else { [0.0, 0.0, 0.0, 1.0] };
            Lighting {
                lights,
                model_ambient,
                local_viewer: pm.local_camera_lighting,
                clamp: !self.cfg.disable_light_clamp,
                material: MaterialParams::from_m3g(m),
            }
        });

        // Per-vertex inputs.
        let count = vb.vertex_count;
        let mut inv_mv = mv;
        if lighting.is_some() {
            inv_mv.invert();
        }
        let vertex_color = |i: usize| -> [f32; 4] {
            let (r, g, b, a): (u8, u8, u8, u8) = match &vb.colors {
                None => {
                    let c = vb.default_color;
                    (
                        ((c >> 16) & 255) as u8,
                        ((c >> 8) & 255) as u8,
                        (c & 255) as u8,
                        (((c >> 24) & 255) as f32 * alpha_factor) as i32 as u8,
                    )
                }
                Some((data, comps)) => {
                    let o = i * comps;
                    let alpha = if *comps == 4 {
                        if alpha_factor == 1.0 {
                            data[o + 3]
                        } else {
                            ((data[o + 3] as f32) * alpha_factor + 0.5) as i32 as u8
                        }
                    } else if alpha_factor == 1.0 {
                        255
                    } else {
                        (255.0f32 * alpha_factor + 0.5) as i32 as u8
                    };
                    (data[o], data[o + 1], data[o + 2], alpha)
                }
            };
            if vb.colors.is_some() {
                // Colour arrays are unpacked by the vertex fetch as `x * (1/255)`.
                let k = 1.0f32 / 255.0;
                [r as f32 * k, g as f32 * k, b as f32 * k, a as f32 * k]
            } else {
                // glColor4ub goes through a table of `x / 255`.
                [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, a as f32 / 255.0]
            }
        };

        // Clip position, fog coordinate and lit colours, per vertex. Every matrix
        // product is a `dp4` chain, as the GL implementation evaluates them.
        let mut clip = vec![[0.0f32; 4]; count];
        let mut fog_coord = vec![0.0f32; count];
        let mut front = vec![[0.0f32; 4]; count];
        let mut back = vec![[0.0f32; 4]; count];
        for i in 0..count {
            let p = [vb.positions[i * 3], vb.positions[i * 3 + 1], vb.positions[i * 3 + 2], 1.0];
            clip[i] = mvp.transform_dp4(p);
            let eye_z = math::dp4(mv.row(2), p);
            fog_coord[i] = eye_z.abs();

            let vc = vertex_color(i);
            match &lighting {
                None => {
                    front[i] = vc;
                    back[i] = vc;
                }
                Some(l) => {
                    let eye = [math::dp4(mv.row(0), p), math::dp4(mv.row(1), p), eye_z];
                    // Normal matrix: inverse transpose of the modelview, then normalised (GL_NORMALIZE).
                    let n = match &vb.normals {
                        Some(n) => [n[i * 3], n[i * 3 + 1], n[i * 3 + 2]],
                        None => [0.0, 0.0, 1.0],
                    };
                    let m = &inv_mv.m;
                    let ne = [
                        math::dp3([m[0], m[4], m[8]], n),
                        math::dp3([m[1], m[5], m[9]], n),
                        math::dp3([m[2], m[6], m[10]], n),
                    ];
                    let len_sq = math::dp3(ne, ne);
                    let ne = if len_sq == 0.0 {
                        ne
                    } else {
                        let inv = 1.0f32 / (len_sq as f64).sqrt() as f32;
                        [ne[0] * inv, ne[1] * inv, ne[2] * inv]
                    };
                    front[i] = l.light_vertex(eye, ne, vc, false);
                    back[i] = if pm.two_sided_lighting { l.light_vertex(eye, ne, vc, true) } else { front[i] };
                }
            }
        }

        // Texture units: matrix, coordinates, texture.
        let mut units: [Option<TexUnit>; TEX_UNITS] = [None; TEX_UNITS];
        let mut uvs: Vec<Option<Vec<[f32; 3]>>> = vec![None; TEX_UNITS];
        for unit in 0..MAX_TEXTURE_UNITS {
            let (Some(tex_id), Some(coords)) = (ap.textures[unit], vb.tex[unit].as_ref()) else {
                continue;
            };
            if unit >= TEX_UNITS {
                self.warn_once("tex-units", "Texture units beyond the fourth are not drawn.");
                continue;
            }
            let t = match scene.kind(tex_id) {
                Kind::Texture2D(t) => t,
                _ => unreachable!("appearances only reference Texture2D"),
            };
            let image_format = match scene.kind(t.image) {
                Kind::Image2D(i) => i.format,
                _ => unreachable!("textures only reference Image2D"),
            };

            let mut tm = t.tr.composite();
            // The renderer zeroes the third bias component before using it.
            gl_translate(&mut tm, coords.scale_bias[1], coords.scale_bias[2], 0.0);
            gl_scale(&mut tm, coords.scale_bias[0], coords.scale_bias[0], coords.scale_bias[0]);
            let mut out = Vec::with_capacity(count);
            for i in 0..count {
                let o = i * coords.components;
                let r = if coords.components == 3 { coords.data[o + 2] } else { 0.0 };
                let v = [coords.data[o], coords.data[o + 1], r, 1.0];
                // Texture matrix * (s, t, r, 1); a 2D lookup needs s, t and q only.
                out.push([math::dp4(tm.row(0), v), math::dp4(tm.row(1), v), math::dp4(tm.row(3), v)]);
            }
            uvs[unit] = Some(out);

            let function = match t.blending {
                FUNC_ADD => 4.0,
                FUNC_BLEND => 3.0,
                FUNC_DECAL => 2.0,
                FUNC_MODULATE => 1.0,
                _ => 0.0,
            };
            let format = match image_format {
                IMAGE_ALPHA => 0.0,
                IMAGE_LUMINANCE => 1.0,
                IMAGE_LUMINANCE_ALPHA => 2.0,
                IMAGE_RGB => 3.0,
                _ => 4.0,
            };
            let mut env = math::fill_float_color(t.blend_color);
            env[3] = 1.0;
            let texture = self.gfx.texture(ctx, scene, tex_id, self.cfg.tex_filter);
            units[unit] = Some(TexUnit { texture, function, format, env_color: env });
        }

        // Expand strips to a triangle list with GL's strip winding and provoking vertex.
        let flat = pm.shading == SHADE_FLAT;
        let mut data: Vec<f32> = Vec::new();
        let mut start = 0usize;
        for &len in &strips.strip_lengths {
            let len = len as usize;
            let strip = &strips.indices[start..start + len];
            start += len;
            for k in 0..len - 2 {
                let (a, b, c) = if k & 1 == 0 {
                    (strip[k], strip[k + 1], strip[k + 2])
                } else {
                    (strip[k + 1], strip[k], strip[k + 2])
                };
                let provoking = strip[k + 2] as usize;
                for &v in &[a, b, c] {
                    let v = v as usize;
                    if v >= count || provoking >= count {
                        return Err(format!("index {v} is outside the {count} vertices of the mesh"));
                    }
                    let src = if flat { provoking } else { v };
                    data.extend_from_slice(&clip[v]);
                    data.extend_from_slice(&front[src]);
                    data.extend_from_slice(&back[src]);
                    for unit in 0..TEX_UNITS {
                        match &uvs[unit] {
                            Some(u) => data.extend_from_slice(&u[v]),
                            None => data.extend_from_slice(&[0.0, 0.0, 0.0]),
                        }
                    }
                    data.push(fog_coord[v]);
                }
            }
        }

        let cull = match pm.culling {
            CULL_NONE => CullFace::Nothing,
            CULL_FRONT => CullFace::Front,
            _ => CullFace::Back,
        };
        let state = RasterState {
            cull,
            front_face: if pm.winding == WINDING_CW { FrontFaceOrder::Clockwise } else { FrontFaceOrder::CounterClockwise },
            depth_test: cm.depth_test,
            depth_write: cm.depth_write,
            color_mask: (cm.color_write, cm.color_write, cm.color_write, cm.alpha_write),
            blending: cm.blending,
            polygon_offset: if cm.depth_offset_factor == 0.0 && cm.depth_offset_units == 0.0 {
                None
            } else {
                Some((cm.depth_offset_factor, cm.depth_offset_units))
            },
        };

        let fog_uniform = fog.map(|f| {
            let mut color = math::fill_float_color(f.color);
            color[3] = 1.0;
            FogUniform {
                mode: if f.mode == FOG_LINEAR { 1.0 } else { 2.0 },
                start: f.near,
                end: f.far,
                density: f.density,
                color,
            }
        });

        self.gfx.draw(
            ctx,
            &DrawCall {
                vertices: &data,
                two_sided: pm.two_sided_lighting && lighting.is_some(),
                alpha_ref: cm.alpha_threshold,
                fog: fog_uniform,
                units,
                state,
            },
        );
        Ok(())
    }
}
