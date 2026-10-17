//! GPU side: one shader, a pipeline cache and the draw call.
//!
//! Everything goes through macroquad's miniquad context. miniquad's
//! `PipelineParams` cannot express "depth test on, depth write off" (it ties the
//! depth test to `depth_write`), nor polygon offset, so those few states are set
//! with raw GL calls right after the pipeline is applied.

use std::collections::HashMap;

use miniquad::gl as raw;
use miniquad::{
    Bindings, BlendFactor, BlendState, BlendValue, BufferId, BufferLayout, BufferSource,
    BufferType, BufferUsage, Comparison, CullFace, Equation, FilterMode, FrontFaceOrder,
    MipmapFilterMode, PassAction, Pipeline, PipelineParams, RenderingBackend, ShaderId, ShaderMeta,
    ShaderSource, TextureAccess, TextureFormat, TextureId, TextureKind, TextureParams,
    TextureSource, TextureWrap, UniformBlockLayout, UniformDesc, UniformType, UniformsSource,
    VertexAttribute, VertexFormat,
};

use crate::scene::*;

pub const TEX_UNITS: usize = 4;
/// Floats per vertex: clip position 4, front colour 4, back colour 4, 4 x texcoord 3, fog coordinate 1.
pub const VERTEX_FLOATS: usize = 4 + 4 + 4 + 3 * TEX_UNITS + 1;

const VERTEX: &str = r#"#version 100
precision highp float;
attribute vec4 a_clip;
attribute vec4 a_cf;
attribute vec4 a_cb;
attribute vec3 a_uv0;
attribute vec3 a_uv1;
attribute vec3 a_uv2;
attribute vec3 a_uv3;
attribute float a_fog;
varying vec4 v_cf;
varying vec4 v_cb;
varying vec3 v_uv0;
varying vec3 v_uv1;
varying vec3 v_uv2;
varying vec3 v_uv3;
varying float v_fog;
void main() {
    gl_Position = a_clip;
    v_cf = a_cf;
    v_cb = a_cb;
    v_uv0 = a_uv0;
    v_uv1 = a_uv1;
    v_uv2 = a_uv2;
    v_uv3 = a_uv3;
    v_fog = a_fog;
}
"#;

// u_tcN = (enabled, function, format, unused); u_teN = texture environment colour.
// function: 0 replace, 1 modulate, 2 decal, 3 blend, 4 add.
// format: 0 alpha, 1 luminance, 2 luminance+alpha, 3 rgb, 4 rgba.
const FRAGMENT: &str = r#"#version 100
precision highp float;
// Without this, sampler2D defaults to lowp and Mesa returns texels at reduced precision.
precision highp sampler2D;
varying vec4 v_cf;
varying vec4 v_cb;
varying vec3 v_uv0;
varying vec3 v_uv1;
varying vec3 v_uv2;
varying vec3 v_uv3;
varying float v_fog;
uniform sampler2D u_tex0;
uniform sampler2D u_tex1;
uniform sampler2D u_tex2;
uniform sampler2D u_tex3;
uniform vec4 u_tc0;
uniform vec4 u_tc1;
uniform vec4 u_tc2;
uniform vec4 u_tc3;
uniform vec4 u_te0;
uniform vec4 u_te1;
uniform vec4 u_te2;
uniform vec4 u_te3;
uniform vec4 u_misc;      // x two sided, y alpha test reference, z fog mode (0 off, 1 linear, 2 exp)
uniform vec4 u_fog;       // x start, y end, z density, w 1 / (end - start)
uniform vec4 u_fog_color;

vec4 unit(vec4 cf, sampler2D s, vec3 uvq, vec4 cfg, vec4 env) {
    if (cfg.x < 0.5) {
        return cf;
    }
    vec4 t = texture2D(s, uvq.xy * (1.0 / uvq.z));
    float fmt = cfg.z;
    float fn = cfg.y;
    bool has_alpha = fmt < 0.5 || (fmt > 1.5 && fmt < 2.5) || fmt > 3.5;
    bool alpha_only = fmt < 0.5;
    vec3 rgb = cf.rgb;
    float a = cf.a;
    // DECAL is only defined for RGB and RGBA textures; Mesa leaves the colour alone otherwise.
    bool passthrough = alpha_only || (fn > 1.5 && fn < 2.5 && fmt > 0.5 && fmt < 2.5);
    if (!passthrough) {
        if (fn < 0.5) {
            rgb = t.rgb;
        } else if (fn < 1.5) {
            rgb = cf.rgb * t.rgb;
        } else if (fn < 2.5) {
            if (has_alpha) {
                rgb = cf.rgb * (1.0 - t.a) + t.rgb * t.a;
            } else {
                rgb = t.rgb;
            }
        } else if (fn < 3.5) {
            rgb = cf.rgb * (1.0 - t.rgb) + env.rgb * t.rgb;
        } else {
            rgb = cf.rgb + t.rgb;
        }
    }
    if (has_alpha) {
        if (fn < 0.5) {
            a = t.a;
        } else if (fn < 1.5 || fn > 2.5) {
            a = cf.a * t.a;
        }
    }
    return clamp(vec4(rgb, a), 0.0, 1.0);
}

void main() {
    vec4 c = v_cf;
    if (u_misc.x > 0.5 && !gl_FrontFacing) {
        c = v_cb;
    }
    c = unit(c, u_tex0, v_uv0, u_tc0, u_te0);
    c = unit(c, u_tex1, v_uv1, u_tc1, u_te1);
    c = unit(c, u_tex2, v_uv2, u_tc2, u_te2);
    c = unit(c, u_tex3, v_uv3, u_tc3, u_te3);
    if (u_misc.z > 0.5) {
        float f;
        if (u_misc.z < 1.5) {
            f = (u_fog.y - v_fog) * u_fog.w;
        } else {
            f = exp(-u_fog.z * v_fog);
        }
        f = clamp(f, 0.0, 1.0);
        c.rgb = f * c.rgb + (1.0 - f) * u_fog_color.rgb;
    }
    if (u_misc.y > 0.0 && c.a < u_misc.y) {
        discard;
    }
    gl_FragColor = c;
}
"#;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Uniforms {
    tc: [[f32; 4]; TEX_UNITS],
    te: [[f32; 4]; TEX_UNITS],
    misc: [f32; 4],
    fog: [f32; 4],
    fog_color: [f32; 4],
}

#[derive(Clone, Copy)]
pub struct TexUnit {
    pub texture: TextureId,
    pub function: f32,
    pub format: f32,
    pub env_color: [f32; 4],
}

#[derive(Clone, Copy)]
pub struct FogUniform {
    pub mode: f32,
    pub start: f32,
    pub end: f32,
    pub density: f32,
    pub color: [f32; 4],
}

#[derive(Clone, Copy, PartialEq)]
pub struct RasterState {
    pub cull: CullFace,
    pub front_face: FrontFaceOrder,
    pub depth_test: bool,
    pub depth_write: bool,
    pub color_mask: (bool, bool, bool, bool),
    /// One of the `BLEND_*` constants.
    pub blending: i32,
    pub polygon_offset: Option<(f32, f32)>,
}

pub struct DrawCall<'a> {
    /// Triangle list, `VERTEX_FLOATS` per vertex.
    pub vertices: &'a [f32],
    pub two_sided: bool,
    pub alpha_ref: f32,
    pub fog: Option<FogUniform>,
    pub units: [Option<TexUnit>; TEX_UNITS],
    pub state: RasterState,
}

/// What a texture was built from, so a changed setting rebuilds it.
#[derive(PartialEq)]
struct TexKey {
    filter: FilterMode,
    wrap_s: TextureWrap,
    wrap_t: TextureWrap,
}

pub struct Gfx {
    shader: ShaderId,
    pipelines: Vec<(RasterState, Pipeline)>,
    textures: HashMap<Id, (TexKey, TextureId)>,
    white: TextureId,
    frame_buffers: Vec<BufferId>,
}

fn blend_state(mode: i32) -> Option<BlendState> {
    use BlendFactor::*;
    use BlendValue::*;
    let (src, dst) = match mode {
        BLEND_ALPHA => (Value(SourceAlpha), OneMinusValue(SourceAlpha)),
        BLEND_ALPHA_ADD => (Value(SourceAlpha), One),
        BLEND_MODULATE => (Value(DestinationColor), Zero),
        BLEND_MODULATE_X2 => (Value(DestinationColor), Value(SourceColor)),
        _ => return None,
    };
    Some(BlendState::new(Equation::Add, src, dst))
}

impl Gfx {
    pub fn new(ctx: &mut dyn RenderingBackend) -> Gfx {
        let mut uniforms = Vec::new();
        for name in [
            "u_tc0", "u_tc1", "u_tc2", "u_tc3", "u_te0", "u_te1", "u_te2", "u_te3",
        ] {
            uniforms.push(UniformDesc::new(name, UniformType::Float4));
        }
        for name in ["u_misc", "u_fog", "u_fog_color"] {
            uniforms.push(UniformDesc::new(name, UniformType::Float4));
        }
        let meta = ShaderMeta {
            uniforms: UniformBlockLayout { uniforms },
            images: vec![
                "u_tex0".into(),
                "u_tex1".into(),
                "u_tex2".into(),
                "u_tex3".into(),
            ],
        };
        let shader = ctx
            .new_shader(
                ShaderSource::Glsl {
                    vertex: VERTEX,
                    fragment: FRAGMENT,
                },
                meta,
            )
            .unwrap_or_else(|e| panic!("shader compilation failed: {e:?}"));
        let white = ctx.new_texture_from_rgba8(1, 1, &[255, 255, 255, 255]);
        Gfx {
            shader,
            pipelines: Vec::new(),
            textures: HashMap::new(),
            white,
            frame_buffers: Vec::new(),
        }
    }

    fn pipeline(&mut self, ctx: &mut dyn RenderingBackend, state: RasterState) -> Pipeline {
        if let Some((_, p)) = self.pipelines.iter().find(|(s, _)| *s == state) {
            return *p;
        }
        let stride = (VERTEX_FLOATS * 4) as i32;
        let attributes = [
            VertexAttribute::new("a_clip", VertexFormat::Float4),
            VertexAttribute::new("a_cf", VertexFormat::Float4),
            VertexAttribute::new("a_cb", VertexFormat::Float4),
            VertexAttribute::new("a_uv0", VertexFormat::Float3),
            VertexAttribute::new("a_uv1", VertexFormat::Float3),
            VertexAttribute::new("a_uv2", VertexFormat::Float3),
            VertexAttribute::new("a_uv3", VertexFormat::Float3),
            VertexAttribute::new("a_fog", VertexFormat::Float1),
        ];
        let params = PipelineParams {
            cull_face: state.cull,
            front_face_order: state.front_face,
            // The real depth state is set with raw GL after apply_pipeline; this
            // only makes miniquad enable the depth test.
            depth_test: Comparison::LessOrEqual,
            depth_write: true,
            color_blend: blend_state(state.blending),
            color_write: state.color_mask,
            ..Default::default()
        };
        let p = ctx.new_pipeline(
            &[BufferLayout {
                stride,
                ..Default::default()
            }],
            &attributes,
            self.shader,
            params,
        );
        self.pipelines.push((state, p));
        p
    }

    /// Returns the GPU texture for a `Texture2D`, building it on first use.
    pub fn texture(
        &mut self,
        ctx: &mut dyn RenderingBackend,
        scene: &Scene,
        id: Id,
        filter_override: Option<i32>,
    ) -> TextureId {
        let t = match scene.kind(id) {
            Kind::Texture2D(t) => t,
            _ => unreachable!("appearances only reference Texture2D"),
        };
        let mut image_filter = t.image_filter;
        if let Some(f) = filter_override {
            image_filter = f;
        }
        // The original never uploads mipmaps, so the base-level filter is all there is.
        let filter = if image_filter == FILTER_LINEAR {
            FilterMode::Linear
        } else {
            FilterMode::Nearest
        };
        let wrap = |w: i32| {
            if w == WRAP_CLAMP {
                TextureWrap::Clamp
            } else {
                TextureWrap::Repeat
            }
        };
        let key = TexKey {
            filter,
            wrap_s: wrap(t.wrap_s),
            wrap_t: wrap(t.wrap_t),
        };

        if let Some((k, tex)) = self.textures.get(&id) {
            if *k == key {
                return *tex;
            }
            ctx.delete_texture(*tex);
        }

        let image = match scene.kind(t.image) {
            Kind::Image2D(i) => i,
            _ => unreachable!("textures only reference Image2D"),
        };
        let mut rgba = Vec::with_capacity((image.width * image.height * 4) as usize);
        let d = &image.data;
        match image.format {
            IMAGE_ALPHA => d
                .iter()
                .for_each(|&a| rgba.extend_from_slice(&[0, 0, 0, a])),
            IMAGE_LUMINANCE => d
                .iter()
                .for_each(|&l| rgba.extend_from_slice(&[l, l, l, 255])),
            IMAGE_LUMINANCE_ALPHA => d
                .chunks(2)
                .for_each(|p| rgba.extend_from_slice(&[p[0], p[0], p[0], p[1]])),
            IMAGE_RGB => d
                .chunks(3)
                .for_each(|p| rgba.extend_from_slice(&[p[0], p[1], p[2], 255])),
            _ => rgba.extend_from_slice(d),
        }
        let tex = ctx.new_texture(
            TextureAccess::Static,
            TextureSource::Bytes(&rgba),
            TextureParams {
                kind: TextureKind::Texture2D,
                width: image.width as u32,
                height: image.height as u32,
                format: TextureFormat::RGBA8,
                wrap: TextureWrap::Repeat,
                min_filter: filter,
                mag_filter: filter,
                mipmap_filter: MipmapFilterMode::None,
                allocate_mipmaps: false,
                sample_count: 1,
            },
        );
        ctx.texture_set_wrap(tex, key.wrap_s, key.wrap_t);
        self.textures.insert(id, (key, tex));
        tex
    }

    /// Starts a frame: clears colour and depth, like `Renderer.clearBackground`.
    pub fn begin(&mut self, ctx: &mut dyn RenderingBackend, clear: (f32, f32, f32, f32)) {
        ctx.begin_default_pass(PassAction::Clear {
            color: Some(clear),
            depth: Some(1.0),
            stencil: None,
        });
        let (w, h) = miniquad::window::screen_size();
        ctx.apply_viewport(0, 0, w as i32, h as i32);
        ctx.apply_scissor_rect(0, 0, w as i32, h as i32);
    }

    pub fn draw(&mut self, ctx: &mut dyn RenderingBackend, call: &DrawCall) {
        let vertex_count = call.vertices.len() / VERTEX_FLOATS;
        if vertex_count == 0 {
            return;
        }
        let vertex_buffer = ctx.new_buffer(
            BufferType::VertexBuffer,
            BufferUsage::Stream,
            BufferSource::slice(call.vertices),
        );
        let indices: Vec<u32> = (0..vertex_count as u32).collect();
        let index_buffer = ctx.new_buffer(
            BufferType::IndexBuffer,
            BufferUsage::Stream,
            BufferSource::slice(&indices),
        );
        self.frame_buffers.push(vertex_buffer);
        self.frame_buffers.push(index_buffer);

        let pipeline = self.pipeline(ctx, call.state);
        ctx.apply_pipeline(&pipeline);

        // SAFETY: plain GL state calls on the context miniquad made current.
        unsafe {
            raw::glEnable(raw::GL_DEPTH_TEST);
            raw::glDepthFunc(if call.state.depth_test {
                raw::GL_LEQUAL
            } else {
                raw::GL_ALWAYS
            });
            raw::glDepthMask(call.state.depth_write as u8);
            match call.state.polygon_offset {
                Some((factor, units)) => {
                    raw::glPolygonOffset(factor, units);
                    raw::glEnable(raw::GL_POLYGON_OFFSET_FILL);
                }
                None => raw::glDisable(raw::GL_POLYGON_OFFSET_FILL),
            }
        }

        let mut u = Uniforms {
            misc: [
                if call.two_sided { 1.0 } else { 0.0 },
                call.alpha_ref,
                call.fog.map(|f| f.mode).unwrap_or(0.0),
                0.0,
            ],
            ..Default::default()
        };
        if let Some(f) = call.fog {
            u.fog = [f.start, f.end, f.density, 1.0 / (f.end - f.start)];
            u.fog_color = f.color;
        }
        let mut images = [self.white; TEX_UNITS];
        for (i, unit) in call.units.iter().enumerate() {
            if let Some(unit) = unit {
                images[i] = unit.texture;
                u.tc[i] = [1.0, unit.function, unit.format, 0.0];
                u.te[i] = unit.env_color;
            }
        }

        ctx.apply_bindings(&Bindings {
            vertex_buffers: vec![vertex_buffer],
            index_buffer,
            images: images.to_vec(),
        });
        ctx.apply_uniforms(UniformsSource::table(&u));
        ctx.draw(0, vertex_count as i32, 1);
    }

    /// Ends the frame: puts GL state back the way macroquad expects it.
    pub fn end(&mut self, ctx: &mut dyn RenderingBackend) {
        // SAFETY: plain GL state calls on the context miniquad made current.
        unsafe {
            let err = raw::glGetError();
            if err != 0 {
                eprintln!("[m3g-viewer] ERROR GL error: {err} (0x{err:x})");
            }
            raw::glDepthMask(1);
            raw::glDisable(raw::GL_POLYGON_OFFSET_FILL);
            raw::glDisable(raw::GL_DEPTH_TEST);
        }
        ctx.end_render_pass();
        for b in self.frame_buffers.drain(..) {
            ctx.delete_buffer(b);
        }
    }
}
