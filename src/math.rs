//! Matrix, vector and quaternion maths.
//!
//! Every routine mirrors the arithmetic order of the Java viewer it replaces
//! (`m3g.math.*`). Floats are `f32` throughout and trigonometry goes through
//! `f64` exactly where the Java code goes through `double`, so that results are
//! bit-for-bit comparable. Do not "simplify" an expression here without
//! re-running the frame comparison.

pub const DEGREES_TO_RADIANS: f64 = 0.017453292519943295;
pub const RADIANS_TO_DEGREES: f64 = 57.29577951308232;

pub fn to_radians(deg: f64) -> f64 {
    deg * DEGREES_TO_RADIANS
}

pub fn to_degrees(rad: f64) -> f64 {
    rad * RADIANS_TO_DEGREES
}

/// `G3DUtils.limit(float)`.
pub fn limit01(v: f32) -> f32 {
    if v >= 0.0 {
        if v <= 1.0 {
            v
        } else {
            1.0
        }
    } else {
        0.0
    }
}

/// `G3DUtils.limit(float, min, max)`.
pub fn limit(v: f32, min: f32, max: f32) -> f32 {
    if v >= min {
        if v <= max {
            v
        } else {
            max
        }
    } else {
        min
    }
}

pub fn limit_i(v: i32, min: i32, max: i32) -> i32 {
    if v >= min {
        if v <= max {
            v
        } else {
            max
        }
    } else {
        min
    }
}

pub fn limit_positive(v: f32) -> f32 {
    if v >= 0.0 {
        v
    } else {
        0.0
    }
}

/// `G3DUtils.round(float)`; Java's `(int)` cast saturates, Rust's `as` does too.
pub fn round(v: f32) -> i32 {
    if v >= 0.0 {
        (v + 0.5) as i32
    } else {
        (v - 0.5) as i32
    }
}

/// `G3DUtils.getFloatColor(color, shift)`.
pub fn float_color(color: i32, shift: u32) -> f32 {
    (((color >> shift) & 255) as f32) / 255.0
}

/// `G3DUtils.fillFloatColor`: ARGB int to `[r, g, b, a]`.
pub fn fill_float_color(argb: i32) -> [f32; 4] {
    [
        (((argb >> 16) & 255) as f32) / 255.0,
        (((argb >> 8) & 255) as f32) / 255.0,
        ((argb & 255) as f32) / 255.0,
        (((argb >> 24) & 255) as f32) / 255.0,
    ]
}

/// `G3DUtils.getIntColor(float[])`.
pub fn int_color(rgba: &[f32]) -> i32 {
    let (red, green, blue, alpha);
    if rgba.len() == 1 {
        blue = 255;
        green = 255;
        red = 255;
        alpha = (limit01(rgba[0]) * 255.0 + 0.5) as i32;
    } else {
        red = (limit01(rgba[0]) * 255.0 + 0.5) as i32;
        green = (limit01(rgba[1]) * 255.0 + 0.5) as i32;
        blue = (limit01(rgba[2]) * 255.0 + 0.5) as i32;
        if rgba.len() != 4 {
            alpha = 255;
        } else {
            alpha = (limit01(rgba[3]) * 255.0 + 0.5) as i32;
        }
    }
    (alpha << 24)
        .wrapping_add(red << 16)
        .wrapping_add(green << 8)
        .wrapping_add(blue)
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec4 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}

impl Vec4 {
    pub const fn new(x: f32, y: f32, z: f32, w: f32) -> Vec4 {
        Vec4 { x, y, z, w }
    }

    pub fn length(&self) -> f32 {
        ((self.x * self.x + self.y * self.y + self.z * self.z) as f64).sqrt() as f32
    }

    pub fn mul(&mut self, f: f32) {
        self.x *= f;
        self.y *= f;
        self.z *= f;
        self.w *= f;
    }

    pub fn sub(&mut self, o: &Vec4) {
        self.x -= o.x;
        self.y -= o.y;
        self.z -= o.z;
        self.w -= o.w;
    }

    pub fn dot(&self, o: &Vec4) -> f32 {
        self.x * o.x + self.y * o.y + self.z * o.z + self.w * o.w
    }

    pub fn cross(a: &Vec4, b: &Vec4) -> Vec4 {
        Vec4 {
            x: a.y * b.z - a.z * b.y,
            y: a.z * b.x - a.x * b.z,
            z: a.x * b.y - a.y * b.x,
            w: 0.0,
        }
    }

    /// Returns false (and leaves the vector alone) when it is too short.
    pub fn normalize(&mut self) -> bool {
        let length = self.x * self.x + self.y * self.y + self.z * self.z + self.w * self.w;
        if length < 1.0E-5 {
            return false;
        }
        let inv = 1.0f32 / (length as f64).sqrt() as f32;
        self.x *= inv;
        self.y *= inv;
        self.z *= inv;
        self.w *= inv;
        true
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quat {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}

impl Quat {
    pub const IDENTITY: Quat = Quat {
        x: 0.0,
        y: 0.0,
        z: 0.0,
        w: 1.0,
    };

    pub const fn new(x: f32, y: f32, z: f32, w: f32) -> Quat {
        Quat { x, y, z, w }
    }

    pub fn from_slice(v: &[f32]) -> Quat {
        assert!(v.len() == 4, "Invalid number of components for quaternion");
        Quat {
            x: v[0],
            y: v[1],
            z: v[2],
            w: v[3],
        }
    }

    pub fn normalize(&mut self) {
        let len_sq = self.x * self.x + self.y * self.y + self.z * self.z + self.w * self.w;
        if len_sq > 1.0E-5 {
            let inv = 1.0f32 / (len_sq as f64).sqrt() as f32;
            self.x *= inv;
            self.y *= inv;
            self.z *= inv;
            self.w *= inv;
        } else {
            self.x = 0.0;
            self.y = 0.0;
            self.z = 0.0;
            self.w = 1.0;
        }
    }

    pub fn set_angle_axis(&mut self, angle: f32, ax: f32, ay: f32, az: f32) {
        let mut axis = Vec4::new(ax, ay, az, 0.0);
        if axis.normalize() {
            let half = to_radians((0.5f32 * angle) as f64) as f32;
            let s = (half as f64).sin() as f32;
            self.x = s * axis.x;
            self.y = s * axis.y;
            self.z = s * axis.z;
            self.w = (half as f64).cos() as f32;
        } else {
            *self = Quat::IDENTITY;
        }
    }

    /// `Quaternion.getAngleAxis`: normalises `self` as a side effect, like the original.
    pub fn get_angle_axis(&mut self) -> [f32; 4] {
        self.normalize();
        let mut out = [0.0f32; 4];
        let sin_sq = 1.0f32 - self.w * self.w;
        if sin_sq > 1.0E-5 {
            let s = (sin_sq as f64).sqrt() as f32;
            out[1] = self.x / s;
            out[2] = self.y / s;
            out[3] = self.z / s;
        } else {
            out[1] = 0.0;
            out[2] = 0.0;
            out[3] = 1.0;
        }
        out[0] = to_degrees((self.w as f64).acos() * 2.0) as f32;
        out
    }

    pub fn mul(&mut self, o: &Quat) {
        let l = *self;
        self.w = l.w * o.w - l.x * o.x - l.y * o.y - l.z * o.z;
        self.x = l.w * o.x + l.x * o.w + l.y * o.z - l.z * o.y;
        self.y = l.w * o.y - l.x * o.z + l.y * o.w + l.z * o.x;
        self.z = l.w * o.z + l.x * o.y - l.y * o.x + l.z * o.w;
    }

    pub fn mul_scalar(&mut self, s: f32) {
        self.x *= s;
        self.y *= s;
        self.z *= s;
        self.w *= s;
    }

    pub fn add(&mut self, o: &Quat) {
        self.x += o.x;
        self.y += o.y;
        self.z += o.z;
        self.w += o.w;
    }

    pub fn sub(&mut self, o: &Quat) {
        self.x -= o.x;
        self.y -= o.y;
        self.z -= o.z;
        self.w -= o.w;
    }

    fn conjugate(&mut self, o: &Quat) {
        self.x = -o.x;
        self.y = -o.y;
        self.z = -o.z;
        self.w = o.w;
    }

    fn dot(&self, o: &Quat) -> f32 {
        self.x * o.x + self.y * o.y + self.z * o.z + self.w * o.w
    }

    /// `this.log(q)`; note the original reads `this.w` (not `q.w`) for the angle.
    fn log(&mut self, q: &Quat) {
        let vl = ((q.x * q.x + q.y * q.y + q.z * q.z) as f64).sqrt() as f32;
        if vl > 1.0E-5 {
            let scale = ((vl as f64).atan2(self.w as f64) / (vl as f64)) as f32;
            self.x = scale * q.x;
            self.y = scale * q.y;
            self.z = scale * q.z;
        } else {
            self.x = 0.0;
            self.y = 0.0;
            self.z = 0.0;
        }
        self.w = 0.0;
    }

    pub fn exp(&mut self, q: &Quat) {
        let angle = ((q.x * q.x + q.y * q.y + q.z * q.z) as f64).sqrt() as f32;
        if angle > 1.0E-5 {
            let scale = ((angle as f64).sin() as f32) / angle;
            self.x = scale * q.x;
            self.y = scale * q.y;
            self.z = scale * q.z;
            self.w = (angle as f64).cos() as f32;
        } else {
            self.x = 0.0;
            self.y = 0.0;
            self.z = 0.0;
            self.w = 1.0;
        }
    }

    pub fn log_diff(&mut self, from: &Quat, to: &Quat) {
        *self = *from;
        let c = *self;
        self.conjugate(&c);
        self.mul(to);
        let s = *self;
        self.log(&s);
    }

    pub fn slerp(&mut self, t: f32, from: &Quat, to: &Quat) {
        let cos_omega = from.dot(to);
        if cos_omega + 1.0 > 1.0E-5 {
            let (from_w, to_w);
            if 1.0f32 - cos_omega > 1.0E-5 {
                let omega = (cos_omega as f64).acos() as f32;
                let sin_omega = (omega as f64).sin() as f32;
                from_w = (((1.0f32 - t) * omega) as f64).sin() as f32 / sin_omega;
                to_w = ((t * omega) as f64).sin() as f32 / sin_omega;
            } else {
                from_w = 1.0f32 - t;
                to_w = t;
            }
            self.x = from_w * from.x + to_w * to.x;
            self.y = from_w * from.y + to_w * to.y;
            self.z = from_w * from.z + to_w * to.z;
            self.w = from_w * from.w + to_w * to.w;
        } else {
            self.x = -from.y;
            self.y = from.x;
            self.z = -from.w;
            self.w = from.z;
            let pi = std::f64::consts::PI;
            let from_w = (((1.0f32 - t) as f64) * pi / 2.0).sin() as f32;
            let to_w = ((t as f64) * pi / 2.0).sin() as f32;
            self.x = from_w * from.x + to_w * self.x;
            self.y = from_w * from.y + to_w * self.y;
            self.z = from_w * from.z + to_w * self.z;
        }
    }

    pub fn squad(&mut self, t: f32, q0: &Quat, ca: &Quat, cb: &Quat, q1: &Quat) {
        let mut outer = Quat::new(0.0, 0.0, 0.0, 0.0);
        let mut inner = Quat::new(0.0, 0.0, 0.0, 0.0);
        outer.slerp(t, q0, q1);
        inner.slerp(t, ca, cb);
        self.slerp(2.0f32 * t * (1.0f32 - t), &outer, &inner);
    }
}

/// `Transform3D`: a row-major 4x4 matrix stored as `m[row * 4 + column]`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mat4 {
    pub m: [f32; 16],
}

impl Default for Mat4 {
    fn default() -> Self {
        Mat4::IDENTITY
    }
}

impl Mat4 {
    pub const IDENTITY: Mat4 = Mat4 {
        m: [
            1.0, 0.0, 0.0, 0.0, //
            0.0, 1.0, 0.0, 0.0, //
            0.0, 0.0, 1.0, 0.0, //
            0.0, 0.0, 0.0, 1.0,
        ],
    };

    pub fn from_array(m: [f32; 16]) -> Mat4 {
        Mat4 { m }
    }

    /// `G3DUtils.Invert4x4`, ported verbatim (cofactor expansion, no singularity check).
    pub fn invert(&mut self) {
        let matrix = &mut self.m;
        let mut pairs = [0.0f32; 12];
        let mut t = [0.0f32; 16];
        let mut c = [0.0f32; 16];

        for i in 0..4 {
            t[i] = matrix[i * 4];
            t[i + 4] = matrix[i * 4 + 1];
            t[i + 8] = matrix[i * 4 + 2];
            t[i + 12] = matrix[i * 4 + 3];
        }

        pairs[0] = t[10] * t[15];
        pairs[1] = t[11] * t[14];
        pairs[2] = t[9] * t[15];
        pairs[3] = t[11] * t[13];
        pairs[4] = t[9] * t[14];
        pairs[5] = t[10] * t[13];
        pairs[6] = t[8] * t[15];
        pairs[7] = t[11] * t[12];
        pairs[8] = t[8] * t[14];
        pairs[9] = t[10] * t[12];
        pairs[10] = t[8] * t[13];
        pairs[11] = t[9] * t[12];
        c[0] = pairs[0] * t[5] + pairs[3] * t[6] + pairs[4] * t[7];
        c[0] -= pairs[1] * t[5] + pairs[2] * t[6] + pairs[5] * t[7];
        c[1] = pairs[1] * t[4] + pairs[6] * t[6] + pairs[9] * t[7];
        c[1] -= pairs[0] * t[4] + pairs[7] * t[6] + pairs[8] * t[7];
        c[2] = pairs[2] * t[4] + pairs[7] * t[5] + pairs[10] * t[7];
        c[2] -= pairs[3] * t[4] + pairs[6] * t[5] + pairs[11] * t[7];
        c[3] = pairs[5] * t[4] + pairs[8] * t[5] + pairs[11] * t[6];
        c[3] -= pairs[4] * t[4] + pairs[9] * t[5] + pairs[10] * t[6];
        c[4] = pairs[1] * t[1] + pairs[2] * t[2] + pairs[5] * t[3];
        c[4] -= pairs[0] * t[1] + pairs[3] * t[2] + pairs[4] * t[3];
        c[5] = pairs[0] * t[0] + pairs[7] * t[2] + pairs[8] * t[3];
        c[5] -= pairs[1] * t[0] + pairs[6] * t[2] + pairs[9] * t[3];
        c[6] = pairs[3] * t[0] + pairs[6] * t[1] + pairs[11] * t[3];
        c[6] -= pairs[2] * t[0] + pairs[7] * t[1] + pairs[10] * t[3];
        c[7] = pairs[4] * t[0] + pairs[9] * t[1] + pairs[10] * t[2];
        c[7] -= pairs[5] * t[0] + pairs[8] * t[1] + pairs[11] * t[2];
        pairs[0] = t[2] * t[7];
        pairs[1] = t[3] * t[6];
        pairs[2] = t[1] * t[7];
        pairs[3] = t[3] * t[5];
        pairs[4] = t[1] * t[6];
        pairs[5] = t[2] * t[5];
        pairs[6] = t[0] * t[7];
        pairs[7] = t[3] * t[4];
        pairs[8] = t[0] * t[6];
        pairs[9] = t[2] * t[4];
        pairs[10] = t[0] * t[5];
        pairs[11] = t[1] * t[4];
        c[8] = pairs[0] * t[13] + pairs[3] * t[14] + pairs[4] * t[15];
        c[8] -= pairs[1] * t[13] + pairs[2] * t[14] + pairs[5] * t[15];
        c[9] = pairs[1] * t[12] + pairs[6] * t[14] + pairs[9] * t[15];
        c[9] -= pairs[0] * t[12] + pairs[7] * t[14] + pairs[8] * t[15];
        c[10] = pairs[2] * t[12] + pairs[7] * t[13] + pairs[10] * t[15];
        c[10] -= pairs[3] * t[12] + pairs[6] * t[13] + pairs[11] * t[15];
        c[11] = pairs[5] * t[12] + pairs[8] * t[13] + pairs[11] * t[14];
        c[11] -= pairs[4] * t[12] + pairs[9] * t[13] + pairs[10] * t[14];
        c[12] = pairs[2] * t[10] + pairs[5] * t[11] + pairs[1] * t[9];
        c[12] -= pairs[4] * t[11] + pairs[0] * t[9] + pairs[3] * t[10];
        c[13] = pairs[8] * t[11] + pairs[0] * t[8] + pairs[7] * t[10];
        c[13] -= pairs[6] * t[10] + pairs[9] * t[11] + pairs[1] * t[8];
        c[14] = pairs[6] * t[9] + pairs[11] * t[11] + pairs[3] * t[8];
        c[14] -= pairs[10] * t[11] + pairs[2] * t[8] + pairs[7] * t[9];
        c[15] = pairs[10] * t[10] + pairs[4] * t[8] + pairs[9] * t[9];
        c[15] -= pairs[8] * t[9] + pairs[11] * t[10] + pairs[5] * t[8];
        let mut det = t[0] * c[0] + t[1] * c[1] + t[2] * c[2] + t[3] * c[3];
        det = 1.0f32 / det;

        for i in 0..16 {
            matrix[i] = c[i] * det;
        }
    }

    pub fn transpose(&mut self) {
        let m = &mut self.m;
        m.swap(1, 4);
        m.swap(2, 8);
        m.swap(3, 12);
        m.swap(7, 13);
        m.swap(11, 14);
        m.swap(6, 9);
    }

    pub fn transposed(&self) -> Mat4 {
        let mut r = *self;
        r.transpose();
        r
    }

    /// `Transform3D.postMultiply`: `self = self * other`, or `other * self` when `pre`.
    pub fn post_multiply(&mut self, other: &Mat4, pre: bool) {
        let mut result = [0.0f32; 16];
        let left = if pre { self.m } else { other.m };
        let right = if pre { other.m } else { self.m };

        for row in 0..4 {
            for column in 0..4 {
                let co = column << 2;
                result[co + row] += left[row] * right[co];
                result[co + row] += left[4 + row] * right[co + 1];
                result[co + row] += left[8 + row] * right[co + 2];
                result[co + row] += left[12 + row] * right[co + 3];
            }
        }
        self.m = result;
    }

    /// Plain `self * other` returning a new matrix.
    pub fn mul(&self, other: &Mat4) -> Mat4 {
        let mut r = *self;
        r.post_multiply(other, false);
        r
    }

    pub fn post_rotate_quat(&mut self, qx: f32, qy: f32, qz: f32, qw: f32) {
        let mut q = Quat::new(qx, qy, qz, qw);
        q.normalize();
        let mut rot = Mat4::IDENTITY;
        let xx = q.x * q.x;
        let xy = q.x * q.y;
        let xz = q.x * q.z;
        let xw = q.x * q.w;
        let yy = q.y * q.y;
        let yz = q.y * q.z;
        let yw = q.y * q.w;
        let zz = q.z * q.z;
        let zw = q.z * q.w;
        let m = &mut rot.m;
        m[0] = 1.0 - 2.0 * (yy + zz);
        m[1] = 2.0 * (xy - zw);
        m[2] = 2.0 * (xz + yw);
        m[4] = 2.0 * (xy + zw);
        m[5] = 1.0 - 2.0 * (xx + zz);
        m[6] = 2.0 * (yz - xw);
        m[8] = 2.0 * (xz - yw);
        m[9] = 2.0 * (yz + xw);
        m[10] = 1.0 - 2.0 * (xx + yy);
        self.post_multiply(&rot, false);
    }

    pub fn post_scale(&mut self, sx: f32, sy: f32, sz: f32) {
        let m = &mut self.m;
        m[0] *= sx;
        m[1] *= sy;
        m[2] *= sz;
        m[4] *= sx;
        m[5] *= sy;
        m[6] *= sz;
        m[8] *= sx;
        m[9] *= sy;
        m[10] *= sz;
        m[12] *= sx;
        m[13] *= sy;
        m[14] *= sz;
    }

    pub fn post_translate(&mut self, tx: f32, ty: f32, tz: f32) {
        let m = &mut self.m;
        m[3] += m[0] * tx + m[1] * ty + m[2] * tz;
        m[7] += m[4] * tx + m[5] * ty + m[6] * tz;
        m[11] += m[8] * tx + m[9] * ty + m[10] * tz;
        m[15] += m[12] * tx + m[13] * ty + m[14] * tz;
    }

    /// Transforms a homogeneous vector in place (`Transform3D.transform(float[])` for one vector).
    pub fn transform4(&self, v: &mut [f32; 4]) {
        let m = &self.m;
        let x = m[0] * v[0] + m[1] * v[1] + m[2] * v[2] + m[3] * v[3];
        let y = m[4] * v[0] + m[5] * v[1] + m[6] * v[2] + m[7] * v[3];
        let z = m[8] * v[0] + m[9] * v[1] + m[10] * v[2] + m[11] * v[3];
        let w = m[12] * v[0] + m[13] * v[1] + m[14] * v[2] + m[15] * v[3];
        v[0] = x;
        v[1] = y;
        v[2] = z;
        v[3] = w;
    }

    pub fn transform_vec(&self, v: &mut Vec4) {
        let mut a = [v.x, v.y, v.z, v.w];
        self.transform4(&mut a);
        v.x = a[0];
        v.y = a[1];
        v.z = a[2];
        v.w = a[3];
    }
}

/// A 4-component dot product evaluated as a fused multiply-add chain:
/// `fma(w, r3, fma(z, r2, fma(y, r1, x * r0)))`.
///
/// This is how Mesa's fixed-function vertex program computes a `DP4` on
/// llvmpipe. It was identified by reading back the depth buffer of the Java
/// reference and trying candidate evaluation orders until every covered pixel
/// matched (see README, "Fidelity"). Plain `x*r0 + y*r1 + ...` differs in the
/// last bit and flips depth ties.
pub fn dp4(row: [f32; 4], v: [f32; 4]) -> f32 {
    let acc = v[0] * row[0];
    let acc = v[1].mul_add(row[1], acc);
    let acc = v[2].mul_add(row[2], acc);
    v[3].mul_add(row[3], acc)
}

/// The 3-component version used for normals.
pub fn dp3(row: [f32; 3], v: [f32; 3]) -> f32 {
    let acc = v[0] * row[0];
    let acc = v[1].mul_add(row[1], acc);
    v[2].mul_add(row[2], acc)
}

impl Mat4 {
    pub fn row(&self, r: usize) -> [f32; 4] {
        [
            self.m[r * 4],
            self.m[r * 4 + 1],
            self.m[r * 4 + 2],
            self.m[r * 4 + 3],
        ]
    }

    /// `M * v` with every output component evaluated by [`dp4`].
    pub fn transform_dp4(&self, v: [f32; 4]) -> [f32; 4] {
        [
            dp4(self.row(0), v),
            dp4(self.row(1), v),
            dp4(self.row(2), v),
            dp4(self.row(3), v),
        ]
    }
}
