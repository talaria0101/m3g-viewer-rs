//! The orbit camera and scene bounds (`OrbitCamera`, `Main.Scene.bounds`).

use crate::math::{Mat4, Quat, Vec4};
use crate::scene::*;

pub struct OrbitCamera {
    yaw: f32,
    pitch: f32,
    distance: f32,
    move_speed: f32,
    target: [f32; 3],
}

const MAX_PITCH: f32 = 88.0;
const DEGREES_PER_PIXEL: f32 = 5.0;

impl OrbitCamera {
    pub fn new() -> OrbitCamera {
        OrbitCamera {
            yaw: 0.0,
            pitch: 25.0,
            distance: 8.0,
            move_speed: 3.0,
            target: [0.0; 3],
        }
    }

    /// `bounds` is `[minX, minY, minZ, maxX, maxY, maxZ]`.
    pub fn frame(&mut self, b: &[f32; 6]) {
        self.target = [
            (b[0] + b[3]) * 0.5,
            (b[1] + b[4]) * 0.5,
            (b[2] + b[5]) * 0.5,
        ];
        let sx = b[3] - b[0];
        let sy = b[4] - b[1];
        let sz = b[5] - b[2];
        let radius = ((sx * sx + sy * sy + sz * sz) as f64).sqrt() as f32 * 0.5;
        self.distance = f32::max(1.0, radius * 2.5);
    }

    pub fn orbit(&mut self, dx: f32, dy: f32) {
        self.yaw += dx * DEGREES_PER_PIXEL;
        self.pitch += dy * DEGREES_PER_PIXEL;
        if self.pitch > MAX_PITCH {
            self.pitch = MAX_PITCH;
        }
        if self.pitch < -MAX_PITCH {
            self.pitch = -MAX_PITCH;
        }
    }

    pub fn zoom(&mut self, delta: f32) {
        self.distance *= 1.0 + delta * 0.1;
        if self.distance < 0.05 {
            self.distance = 0.05;
        }
    }

    pub fn pan(&mut self, right: f32, up: f32) {
        let f = self.forward();
        let u = self.up();
        self.target[0] += (u.x * up - f.x * right) * self.move_speed * 0.05;
        self.target[1] += (u.y * up - f.y * right) * self.move_speed * 0.05;
        self.target[2] += (u.z * up - f.z * right) * self.move_speed * 0.05;
    }

    fn orientation(&self) -> Quat {
        let mut yaw = Quat::new(0.0, 0.0, 0.0, 0.0);
        yaw.set_angle_axis(self.yaw, 0.0, 1.0, 0.0);
        let mut tilt = Quat::new(0.0, 0.0, 0.0, 0.0);
        tilt.set_angle_axis(self.pitch, 1.0, 0.0, 0.0);
        let mut q = Quat::new(0.0, 0.0, 0.0, 1.0);
        q.mul(&yaw);
        q.mul(&tilt);
        q.normalize();
        q
    }

    fn rotate(q: &Quat, x: f32, y: f32, z: f32) -> Vec4 {
        let tx = 2.0 * (q.y * z - q.z * y);
        let ty = 2.0 * (q.z * x - q.x * z);
        let tz = 2.0 * (q.x * y - q.y * x);
        Vec4::new(
            x + q.w * tx + (q.y * tz - q.z * ty),
            y + q.w * ty + (q.z * tx - q.x * tz),
            z + q.w * tz + (q.x * ty - q.y * tx),
            0.0,
        )
    }

    pub fn forward(&self) -> Vec4 {
        Self::rotate(&self.orientation(), 0.0, 0.0, -1.0)
    }

    pub fn eye(&self) -> Vec4 {
        let f = self.forward();
        Vec4::new(
            self.target[0] - f.x * self.distance,
            self.target[1] - f.y * self.distance,
            self.target[2] - f.z * self.distance,
            1.0,
        )
    }

    pub fn up(&self) -> Vec4 {
        let f = self.forward();
        let reference = if f.y.abs() > 0.99 {
            Vec4::new(1.0, 0.0, 0.0, 0.0)
        } else {
            Vec4::new(0.0, 1.0, 0.0, 0.0)
        };
        let mut side = Vec4::cross(&f, &reference);
        side.normalize();
        let mut u = Vec4::cross(&side, &f);
        u.normalize();
        u
    }

    /// Camera-to-world transform; the renderer inverts it.
    pub fn view_matrix(&self) -> Mat4 {
        let o = self.orientation();
        let e = self.eye();
        let mut t = Mat4::IDENTITY;
        t.post_translate(e.x, e.y, e.z);
        t.post_rotate_quat(o.x, o.y, o.z, o.w);
        t
    }
}

/// `OrbitCamera.sceneCameraTransform`: the active camera's *local* composite transform.
pub fn scene_camera_transform(scene: &Scene, world: Option<Id>) -> Option<Mat4> {
    let w = world?;
    match scene.kind(w) {
        Kind::World(wd) => wd.active_camera.map(|c| scene.composite_transform(c)),
        _ => None,
    }
}

fn collect_nodes(scene: &Scene, n: Id, out: &mut Vec<Id>) {
    out.push(n);
    if let Kind::Group(g) = scene.kind(n) {
        for &c in &g.children {
            collect_nodes(scene, c, out);
        }
    } else if let Kind::World(w) = scene.kind(n) {
        for &c in &w.group.children {
            collect_nodes(scene, c, out);
        }
    }
}

/// Axis-aligned bounds of all mesh geometry, each mesh placed by its own
/// local transform only (not its ancestors'), as the original viewer does.
pub fn bounds(scene: &Scene, root: Id) -> [f32; 6] {
    let mut nodes = Vec::new();
    collect_nodes(scene, root, &mut nodes);

    let mut min = [f32::MAX; 3];
    let mut max = [-f32::MAX; 3];

    for n in nodes {
        let Some(mesh) = scene.mesh(n) else { continue };
        let vb = match scene.kind(mesh.vertices) {
            Kind::VertexBuffer(v) => v,
            _ => continue,
        };
        let Some(pos_id) = vb.positions else { continue };
        let array = match scene.kind(pos_id) {
            Kind::VertexArray(a) => a,
            _ => continue,
        };
        let scale = vb.pos_scale_bias[0];
        let bias = [
            vb.pos_scale_bias[1],
            vb.pos_scale_bias[2],
            vb.pos_scale_bias[3],
        ];
        let count = array.vertex_count;

        let local = scene.composite_transform(n);
        for v in 0..count {
            let mut p = [0.0f32; 4];
            for k in 0..3 {
                p[k] = array.get(v * array.component_count + k) * scale + bias[k];
            }
            p[3] = 1.0;
            local.transform4(&mut p);
            for k in 0..3 {
                min[k] = f32::min(min[k], p[k]);
                max[k] = f32::max(max[k], p[k]);
            }
        }
    }

    if min[0] > max[0] {
        return [-0.5, -0.5, -0.5, 0.5, 0.5, 0.5];
    }
    [min[0], min[1], min[2], max[0], max[1], max[2]]
}
