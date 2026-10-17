//! OpenGL fixed-function vertex lighting (GL 2.1, section 2.14), evaluated on the CPU.
//!
//! The viewer this replaces drew with the fixed-function pipeline, so "the same
//! render" means this exact model: per-vertex colour, Gouraud or flat
//! interpolated, with the primary colour clamped to [0, 1] unless the clamp is
//! disabled. Doing it here (rather than in a shader) lets flat shading use the
//! provoking vertex, which GLSL 100 cannot express.

use crate::math::{self, Mat4};
use crate::scene::*;

#[derive(Clone, Debug)]
pub struct Light {
    /// Eye-space position; `w == 0` marks a directional light.
    pub position: [f32; 4],
    pub ambient: [f32; 3],
    pub diffuse: [f32; 3],
    pub specular: [f32; 3],
    pub attenuation: [f32; 3],
    pub spot_direction: [f32; 3],
    /// 180 means "not a spot light".
    pub spot_cutoff: f32,
    pub spot_exponent: f32,
}

impl Light {
    /// Builds the GL light state `Renderer.setupLights` sets for one M3G light.
    ///
    /// `view_light` is camera-from-light: view matrix times the light's world transform.
    pub fn from_m3g(l: &LightData, view_light: &Mat4) -> Light {
        let mut position = if l.mode == LIGHT_DIRECTIONAL { [0.0, 0.0, 1.0, 0.0] } else { [0.0, 0.0, 0.0, 1.0] };
        view_light.transform4(&mut position);

        let mut color = math::fill_float_color(l.color);
        color[0] *= l.intensity;
        color[1] *= l.intensity;
        color[2] *= l.intensity;
        let rgb = [color[0], color[1], color[2]];

        let (ambient, diffuse, specular) = if l.mode == LIGHT_AMBIENT {
            (rgb, [0.0; 3], [0.0; 3])
        } else {
            ([0.0; 3], rgb, rgb)
        };

        let mut light = Light {
            position,
            ambient,
            diffuse,
            specular,
            attenuation: [1.0, 0.0, 0.0],
            spot_direction: [0.0, 0.0, -1.0],
            spot_cutoff: 180.0,
            spot_exponent: 0.0,
        };

        if l.mode == LIGHT_SPOT {
            // GL transforms a spot direction by the inverse transpose of the modelview.
            let mut inv = *view_light;
            inv.invert();
            let d = [0.0f32, 0.0, -1.0];
            let m = &inv.m;
            // transpose(inv) * d, row i of the transpose is column i of inv.
            light.spot_direction = [
                m[0] * d[0] + m[4] * d[1] + m[8] * d[2],
                m[1] * d[0] + m[5] * d[1] + m[9] * d[2],
                m[2] * d[0] + m[6] * d[1] + m[10] * d[2],
            ];
            light.spot_cutoff = l.spot_angle;
            light.spot_exponent = l.spot_exponent;
        }
        if l.mode == LIGHT_SPOT || l.mode == LIGHT_OMNI {
            light.attenuation = [l.constant, l.linear, l.quadratic];
        }
        light
    }
}

#[derive(Clone, Debug)]
pub struct MaterialParams {
    pub ambient: [f32; 4],
    pub diffuse: [f32; 4],
    pub emission: [f32; 4],
    pub specular: [f32; 4],
    pub shininess: f32,
    pub color_tracking: bool,
}

impl MaterialParams {
    pub fn from_m3g(m: &MaterialData) -> MaterialParams {
        MaterialParams {
            ambient: math::fill_float_color(m.ambient),
            diffuse: math::fill_float_color(m.diffuse),
            emission: math::fill_float_color(m.emissive),
            specular: math::fill_float_color(m.specular),
            shininess: m.shininess,
            color_tracking: m.vertex_color_tracking,
        }
    }
}

pub struct Lighting {
    pub lights: Vec<Light>,
    pub model_ambient: [f32; 4],
    pub local_viewer: bool,
    pub clamp: bool,
    pub material: MaterialParams,
}

fn dot3(a: &[f32; 3], b: &[f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn normalize3(v: [f32; 3]) -> [f32; 3] {
    let len_sq = dot3(&v, &v);
    if len_sq == 0.0 {
        return v;
    }
    let inv = 1.0f32 / (len_sq as f64).sqrt() as f32;
    [v[0] * inv, v[1] * inv, v[2] * inv]
}

impl Lighting {
    /// Lit RGBA for one vertex. `eye` and `normal` are in eye space; `normal`
    /// must already be normalised. `back` evaluates the back face (flipped normal).
    pub fn light_vertex(&self, eye: [f32; 3], normal: [f32; 3], vertex_color: [f32; 4], back: bool) -> [f32; 4] {
        let m = &self.material;
        let (mat_ambient, mat_diffuse) = if m.color_tracking {
            (vertex_color, vertex_color)
        } else {
            (m.ambient, m.diffuse)
        };

        let n = if back { [-normal[0], -normal[1], -normal[2]] } else { normal };

        let mut color = [
            m.emission[0] + self.model_ambient[0] * mat_ambient[0],
            m.emission[1] + self.model_ambient[1] * mat_ambient[1],
            m.emission[2] + self.model_ambient[2] * mat_ambient[2],
        ];

        for light in &self.lights {
            let (vp, attenuation) = if light.position[3] != 0.0 {
                let d = [
                    light.position[0] - eye[0],
                    light.position[1] - eye[1],
                    light.position[2] - eye[2],
                ];
                let dist = (dot3(&d, &d) as f64).sqrt() as f32;
                let vp = if dist == 0.0 { d } else { [d[0] / dist, d[1] / dist, d[2] / dist] };
                let a = light.attenuation;
                (vp, 1.0f32 / (a[0] + a[1] * dist + a[2] * dist * dist))
            } else {
                (normalize3([light.position[0], light.position[1], light.position[2]]), 1.0f32)
            };

            let mut spot = 1.0f32;
            if light.spot_cutoff != 180.0 {
                let dir = normalize3(light.spot_direction);
                let minus_vp = [-vp[0], -vp[1], -vp[2]];
                let spot_dot = dot3(&minus_vp, &dir);
                let cos_cutoff = (math::to_radians(light.spot_cutoff as f64)).cos() as f32;
                if spot_dot < cos_cutoff || spot_dot < 0.0 {
                    spot = 0.0;
                } else {
                    spot = (spot_dot as f64).powf(light.spot_exponent as f64) as f32;
                }
            }

            let n_dot_vp = dot3(&n, &vp).max(0.0);
            let factor = attenuation * spot;

            let mut sum = [0.0f32; 3];
            for c in 0..3 {
                sum[c] += light.ambient[c] * mat_ambient[c];
                sum[c] += n_dot_vp * light.diffuse[c] * mat_diffuse[c];
            }
            if n_dot_vp > 0.0 {
                let viewer = if self.local_viewer {
                    normalize3([-eye[0], -eye[1], -eye[2]])
                } else {
                    [0.0, 0.0, 1.0]
                };
                let h = normalize3([vp[0] + viewer[0], vp[1] + viewer[1], vp[2] + viewer[2]]);
                let n_dot_h = dot3(&n, &h);
                if n_dot_h > 0.0 {
                    let s = (n_dot_h as f64).powf(m.shininess as f64) as f32;
                    for c in 0..3 {
                        sum[c] += s * light.specular[c] * m.specular[c];
                    }
                }
            }
            for c in 0..3 {
                color[c] += factor * sum[c];
            }
        }

        let alpha = mat_diffuse[3];
        let out = [color[0], color[1], color[2], alpha];
        if self.clamp {
            [math::limit01(out[0]), math::limit01(out[1]), math::limit01(out[2]), math::limit01(out[3])]
        } else {
            out
        }
    }
}
