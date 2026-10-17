//! The M3G (JSR-184) object graph.
//!
//! Objects live in an arena and refer to each other by [`Id`]. Shared objects
//! (an `Appearance` used by two meshes, say) are therefore shared for real, and
//! animation that mutates one is seen through every user, as in the original.

use crate::math::{Mat4, Quat};

pub type Id = usize;
pub type Result<T> = std::result::Result<T, String>;

pub const MAX_TEXTURE_UNITS: usize = 10;
pub const MAX_TRANSFORMS_PER_VERTEX: usize = 4;

// Animation target properties (AnimationTrack constants).
pub mod prop {
    pub const ALPHA: i32 = 256;
    pub const AMBIENT_COLOR: i32 = 257;
    pub const COLOR: i32 = 258;
    pub const CROP: i32 = 259;
    pub const DENSITY: i32 = 260;
    pub const DIFFUSE_COLOR: i32 = 261;
    pub const EMISSIVE_COLOR: i32 = 262;
    pub const FAR_DISTANCE: i32 = 263;
    pub const FIELD_OF_VIEW: i32 = 264;
    pub const INTENSITY: i32 = 265;
    pub const MORPH_WEIGHTS: i32 = 266;
    pub const NEAR_DISTANCE: i32 = 267;
    pub const ORIENTATION: i32 = 268;
    pub const PICKABILITY: i32 = 269;
    pub const SCALE: i32 = 270;
    pub const SHININESS: i32 = 271;
    pub const SPECULAR_COLOR: i32 = 272;
    pub const SPOT_ANGLE: i32 = 273;
    pub const SPOT_EXPONENT: i32 = 274;
    pub const TRANSLATION: i32 = 275;
    pub const VISIBILITY: i32 = 276;
}

// Enumerations, with the numeric values the file format and the original use.
pub const IMAGE_ALPHA: i32 = 96;
pub const IMAGE_LUMINANCE: i32 = 97;
pub const IMAGE_LUMINANCE_ALPHA: i32 = 98;
pub const IMAGE_RGB: i32 = 99;
pub const IMAGE_RGBA: i32 = 100;

pub const CULL_BACK: i32 = 160;
pub const CULL_FRONT: i32 = 161;
pub const CULL_NONE: i32 = 162;
pub const SHADE_FLAT: i32 = 164;
pub const SHADE_SMOOTH: i32 = 165;
pub const WINDING_CCW: i32 = 168;
pub const WINDING_CW: i32 = 169;

pub const BLEND_ALPHA: i32 = 64;
pub const BLEND_ALPHA_ADD: i32 = 65;
pub const BLEND_MODULATE: i32 = 66;
pub const BLEND_MODULATE_X2: i32 = 67;
pub const BLEND_REPLACE: i32 = 68;

pub const FILTER_BASE_LEVEL: i32 = 208;
pub const FILTER_LINEAR: i32 = 209;
pub const FILTER_NEAREST: i32 = 210;
pub const FUNC_ADD: i32 = 224;
pub const FUNC_BLEND: i32 = 225;
pub const FUNC_DECAL: i32 = 226;
pub const FUNC_MODULATE: i32 = 227;
pub const FUNC_REPLACE: i32 = 228;
pub const WRAP_CLAMP: i32 = 240;
pub const WRAP_REPEAT: i32 = 241;

pub const FOG_EXPONENTIAL: i32 = 80;
pub const FOG_LINEAR: i32 = 81;

pub const LIGHT_AMBIENT: i32 = 128;
pub const LIGHT_DIRECTIONAL: i32 = 129;
pub const LIGHT_OMNI: i32 = 130;
pub const LIGHT_SPOT: i32 = 131;

pub const CAMERA_GENERIC: i32 = 48;
pub const CAMERA_PARALLEL: i32 = 49;
pub const CAMERA_PERSPECTIVE: i32 = 50;

pub const KF_LINEAR: i32 = 176;
pub const KF_SLERP: i32 = 177;
pub const KF_SPLINE: i32 = 178;
pub const KF_SQUAD: i32 = 179;
pub const KF_STEP: i32 = 180;
pub const KF_CONSTANT: i32 = 192;
pub const KF_LOOP: i32 = 193;

pub const MATERIAL_AMBIENT: i32 = 1024;
pub const MATERIAL_DIFFUSE: i32 = 2048;
pub const MATERIAL_EMISSIVE: i32 = 4096;
pub const MATERIAL_SPECULAR: i32 = 8192;

/// `Transformable`: translation, scale, orientation and a free matrix.
#[derive(Clone, Debug)]
pub struct Transformable {
    pub translation: [f32; 3],
    pub scale: [f32; 3],
    pub rotation: Quat,
    pub transform: Mat4,
}

impl Default for Transformable {
    fn default() -> Self {
        Transformable {
            translation: [0.0; 3],
            scale: [1.0; 3],
            rotation: Quat::IDENTITY,
            transform: Mat4::IDENTITY,
        }
    }
}

impl Transformable {
    pub fn set_orientation(&mut self, angle: f32, ax: f32, ay: f32, az: f32) -> Result<()> {
        if angle != 0.0 && ax == 0.0 && ay == 0.0 && az == 0.0 {
            return Err("setOrientation: zero axis with non-zero angle".into());
        }
        self.rotation.set_angle_axis(angle, ax, ay, az);
        Ok(())
    }

    pub fn get_orientation(&mut self) -> [f32; 4] {
        self.rotation.get_angle_axis()
    }

    /// `getCompositeTransform`: T * R * S * M.
    pub fn composite(&self) -> Mat4 {
        let mut r = Mat4::IDENTITY;
        r.post_translate(
            self.translation[0],
            self.translation[1],
            self.translation[2],
        );
        r.post_rotate_quat(
            self.rotation.x,
            self.rotation.y,
            self.rotation.z,
            self.rotation.w,
        );
        r.post_scale(self.scale[0], self.scale[1], self.scale[2]);
        r.post_multiply(&self.transform, false);
        r
    }

    pub fn update_property(&mut self, property: i32, v: &[f32]) -> bool {
        match property {
            prop::ORIENTATION => {
                self.rotation = Quat::from_slice(v);
                self.rotation.normalize();
                true
            }
            prop::SCALE => {
                if v.len() == 1 {
                    self.scale = [v[0]; 3];
                } else {
                    self.scale = [v[0], v[1], v[2]];
                }
                true
            }
            prop::TRANSLATION => {
                self.translation = [v[0], v[1], v[2]];
                true
            }
            _ => false,
        }
    }
}

/// State every scene-graph node carries.
#[derive(Clone, Debug)]
pub struct NodeBase {
    pub tr: Transformable,
    pub parent: Option<Id>,
    pub rendering: bool,
    pub picking: bool,
    pub alpha_factor: f32,
    pub scope: i32,
}

impl Default for NodeBase {
    fn default() -> Self {
        NodeBase {
            tr: Transformable::default(),
            parent: None,
            rendering: true,
            picking: true,
            alpha_factor: 1.0,
            scope: -1,
        }
    }
}

#[derive(Clone, Debug)]
pub struct GroupData {
    pub node: NodeBase,
    pub children: Vec<Id>,
}

#[derive(Clone, Debug)]
pub struct WorldData {
    pub group: GroupData,
    pub active_camera: Option<Id>,
    pub background: Option<Id>,
    pub world_time: i32,
}

#[derive(Clone, Debug)]
pub struct MeshData {
    pub node: NodeBase,
    pub vertices: Id,
    pub submeshes: Vec<Id>,
    pub appearances: Vec<Option<Id>>,
}

#[derive(Clone, Debug)]
pub struct MorphingData {
    pub mesh: MeshData,
    pub targets: Vec<Id>,
    pub weights: Vec<f32>,
    pub base_weight: f32,
}

#[derive(Clone, Debug)]
pub struct BoneTransform {
    pub bone: Id,
    pub to_bone: Mat4,
}

#[derive(Clone, Debug)]
pub struct SkinnedData {
    pub mesh: MeshData,
    pub skeleton: Id,
    pub bones: Vec<BoneTransform>,
    /// `MAX_TRANSFORMS_PER_VERTEX` slots per vertex; 0 means unused, n means `bones[n - 1]`.
    pub vtx_bones: Vec<i32>,
    pub vtx_weights: Vec<i32>,
}

#[derive(Clone, Debug)]
pub struct SpriteData {
    pub node: NodeBase,
    pub scaled: bool,
    pub image: Id,
    pub appearance: Option<Id>,
    pub crop: [i32; 4],
}

#[derive(Clone, Debug)]
pub struct CameraData {
    pub node: NodeBase,
    pub projection_type: i32,
    pub generic: Mat4,
    pub projection: [f32; 4],
}

impl CameraData {
    pub fn new(node: NodeBase) -> CameraData {
        CameraData {
            node,
            projection_type: CAMERA_GENERIC,
            generic: Mat4::IDENTITY,
            projection: [2.0, 1.0, -1.0, 1.0],
        }
    }

    pub fn set_perspective(&mut self, fovy: f32, aspect: f32, near: f32, far: f32) -> Result<()> {
        if fovy > 0.0 && fovy < 180.0 && aspect > 0.0 && near > 0.0 && far > 0.0 {
            self.projection = [fovy, aspect, near, far];
            self.projection_type = CAMERA_PERSPECTIVE;
            Ok(())
        } else {
            Err("Camera.setPerspective: invalid arguments".into())
        }
    }

    pub fn set_parallel(&mut self, height: f32, aspect: f32, near: f32, far: f32) -> Result<()> {
        if height > 0.0 && aspect > 0.0 {
            self.projection = [height, aspect, near, far];
            self.projection_type = CAMERA_PARALLEL;
            Ok(())
        } else {
            Err("Camera.setParallel: invalid arguments".into())
        }
    }

    /// `Camera.getProjection(Transform)`: the projection matrix in M3G (row-major) layout.
    pub fn projection_matrix(&self) -> Result<Mat4> {
        let p = &self.projection;
        if self.projection_type == CAMERA_PARALLEL {
            let height = p[0];
            let width = p[1] * height;
            let depth = p[3] - p[2];
            if depth == 0.0 {
                return Err("near == far".into());
            }
            let mut m = [0.0f32; 16];
            m[0] = 2.0 / width;
            m[5] = 2.0 / height;
            m[10] = -2.0 / depth;
            m[11] = -(p[2] + p[3]) / depth;
            m[15] = 1.0;
            Ok(Mat4::from_array(m))
        } else if self.projection_type == CAMERA_PERSPECTIVE {
            let height = crate::math::to_radians((p[0] / 2.0) as f64).tan() as f32;
            let width = p[1] * height;
            let depth = p[3] - p[2];
            if depth == 0.0 {
                return Err("near == far".into());
            }
            let mut m = [0.0f32; 16];
            m[0] = 1.0 / width;
            m[5] = 1.0 / height;
            m[10] = -(p[2] + p[3]) / depth;
            m[11] = -2.0 * p[2] * p[3] / depth;
            m[14] = -1.0;
            Ok(Mat4::from_array(m))
        } else {
            Ok(self.generic)
        }
    }
}

#[derive(Clone, Debug)]
pub struct LightData {
    pub node: NodeBase,
    pub mode: i32,
    pub color: i32,
    pub intensity: f32,
    pub constant: f32,
    pub linear: f32,
    pub quadratic: f32,
    pub spot_angle: f32,
    pub spot_exponent: f32,
}

impl LightData {
    pub fn new(node: NodeBase) -> LightData {
        LightData {
            node,
            mode: LIGHT_DIRECTIONAL,
            color: 0x00FF_FFFF,
            intensity: 1.0,
            constant: 1.0,
            linear: 0.0,
            quadratic: 0.0,
            spot_angle: 45.0,
            spot_exponent: 0.0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct AppearanceData {
    pub layer: i32,
    pub polygon_mode: Option<Id>,
    pub compositing_mode: Option<Id>,
    pub material: Option<Id>,
    pub fog: Option<Id>,
    pub textures: [Option<Id>; MAX_TEXTURE_UNITS],
}

impl Default for AppearanceData {
    fn default() -> Self {
        AppearanceData {
            layer: 0,
            polygon_mode: None,
            compositing_mode: None,
            material: None,
            fog: None,
            textures: [None; MAX_TEXTURE_UNITS],
        }
    }
}

#[derive(Clone, Debug)]
pub struct BackgroundData {
    pub color_clear: bool,
    pub depth_clear: bool,
    pub color: i32,
    pub image: Option<Id>,
    pub mode_x: i32,
    pub mode_y: i32,
    pub crop: [i32; 4],
}

impl Default for BackgroundData {
    fn default() -> Self {
        BackgroundData {
            color_clear: true,
            depth_clear: true,
            color: 0,
            image: None,
            mode_x: 32,
            mode_y: 32,
            crop: [0; 4],
        }
    }
}

#[derive(Clone, Debug)]
pub struct CompositingModeData {
    pub blending: i32,
    pub alpha_threshold: f32,
    pub depth_offset_factor: f32,
    pub depth_offset_units: f32,
    pub depth_test: bool,
    pub depth_write: bool,
    pub color_write: bool,
    pub alpha_write: bool,
}

impl Default for CompositingModeData {
    fn default() -> Self {
        CompositingModeData {
            blending: BLEND_REPLACE,
            alpha_threshold: 0.0,
            depth_offset_factor: 0.0,
            depth_offset_units: 0.0,
            depth_test: true,
            depth_write: true,
            color_write: true,
            alpha_write: true,
        }
    }
}

#[derive(Clone, Debug)]
pub struct FogData {
    pub mode: i32,
    pub density: f32,
    pub near: f32,
    pub far: f32,
    pub color: i32,
}

impl Default for FogData {
    fn default() -> Self {
        FogData {
            mode: FOG_LINEAR,
            density: 1.0,
            near: 0.0,
            far: 1.0,
            color: 0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct PolygonModeData {
    pub culling: i32,
    pub shading: i32,
    pub winding: i32,
    pub two_sided_lighting: bool,
    pub local_camera_lighting: bool,
    pub perspective_correction: bool,
}

impl Default for PolygonModeData {
    fn default() -> Self {
        PolygonModeData {
            culling: CULL_BACK,
            shading: SHADE_SMOOTH,
            winding: WINDING_CCW,
            two_sided_lighting: false,
            local_camera_lighting: false,
            perspective_correction: false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct MaterialData {
    pub vertex_color_tracking: bool,
    pub ambient: i32,
    pub diffuse: i32,
    pub emissive: i32,
    pub specular: i32,
    pub shininess: f32,
}

impl Default for MaterialData {
    fn default() -> Self {
        MaterialData {
            vertex_color_tracking: false,
            ambient: 3355443,
            diffuse: -3355444,
            emissive: 0,
            specular: 0,
            shininess: 0.0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Texture2DData {
    pub tr: Transformable,
    pub image: Id,
    pub wrap_s: i32,
    pub wrap_t: i32,
    pub level_filter: i32,
    pub image_filter: i32,
    pub blending: i32,
    pub blend_color: i32,
}

#[derive(Clone, Debug)]
pub struct Image2DData {
    pub format: i32,
    pub width: i32,
    pub height: i32,
    /// Tightly packed pixels, `bytes_per_pixel(format)` bytes each, top row first.
    pub data: Vec<u8>,
}

pub fn bytes_per_pixel(format: i32) -> Result<usize> {
    match format {
        IMAGE_ALPHA | IMAGE_LUMINANCE => Ok(1),
        IMAGE_LUMINANCE_ALPHA => Ok(2),
        IMAGE_RGB => Ok(3),
        IMAGE_RGBA => Ok(4),
        _ => Err(format!("invalid Image2D format {format}")),
    }
}

#[derive(Clone, Debug)]
pub enum ArrayData {
    Bytes(Vec<i8>),
    Shorts(Vec<i16>),
}

#[derive(Clone, Debug)]
pub struct VertexArrayData {
    pub vertex_count: usize,
    pub component_count: usize,
    pub data: ArrayData,
}

impl VertexArrayData {
    /// Component size in bytes: 1 or 2 (the original calls it `componentType`).
    pub fn component_size(&self) -> i32 {
        match self.data {
            ArrayData::Bytes(_) => 1,
            ArrayData::Shorts(_) => 2,
        }
    }

    pub fn get(&self, index: usize) -> f32 {
        match &self.data {
            ArrayData::Bytes(b) => b[index] as f32,
            ArrayData::Shorts(s) => s[index] as f32,
        }
    }
}

#[derive(Clone, Debug)]
pub struct VertexBufferData {
    pub vertex_count: usize,
    pub positions: Option<Id>,
    pub normals: Option<Id>,
    pub colors: Option<Id>,
    pub tex_coords: [Option<Id>; MAX_TEXTURE_UNITS],
    pub default_color: i32,
    /// `[scale, bias x, bias y, bias z]`.
    pub pos_scale_bias: [f32; 4],
    pub uv_scale_bias: [[f32; 4]; MAX_TEXTURE_UNITS],
}

impl Default for VertexBufferData {
    fn default() -> Self {
        VertexBufferData {
            vertex_count: 0,
            positions: None,
            normals: None,
            colors: None,
            tex_coords: [None; MAX_TEXTURE_UNITS],
            default_color: -1,
            pos_scale_bias: [1.0, 0.0, 0.0, 0.0],
            uv_scale_bias: [[0.0; 4]; MAX_TEXTURE_UNITS],
        }
    }
}

#[derive(Clone, Debug)]
pub struct TriangleStripData {
    pub indices: Vec<i32>,
    pub strip_lengths: Vec<i32>,
}

#[derive(Clone, Debug)]
pub struct ControllerData {
    pub active_start: i32,
    pub active_end: i32,
    pub ref_world_time: i32,
    pub ref_sequence_time: f32,
    pub weight: f32,
    pub speed: f32,
}

impl Default for ControllerData {
    fn default() -> Self {
        ControllerData {
            active_start: 0,
            active_end: 0,
            ref_world_time: 0,
            ref_sequence_time: 0.0,
            weight: 1.0,
            speed: 1.0,
        }
    }
}

impl ControllerData {
    pub fn set_active_interval(&mut self, start: i32, end: i32) -> Result<()> {
        if start > end {
            return Err("AnimationController.setActiveInterval: start > end".into());
        }
        self.active_start = start;
        self.active_end = end;
        Ok(())
    }

    pub fn get_position(&self, world_time: i32) -> f32 {
        self.ref_sequence_time + self.speed * (world_time.wrapping_sub(self.ref_world_time) as f32)
    }

    pub fn set_speed(&mut self, speed: f32, world_time: i32) {
        self.ref_sequence_time = self.get_position(world_time);
        self.ref_world_time = world_time;
        self.speed = speed;
    }

    pub fn set_position(&mut self, sequence_time: f32, world_time: i32) {
        self.ref_sequence_time = sequence_time;
        self.ref_world_time = world_time;
    }

    pub fn set_weight(&mut self, weight: f32) -> Result<()> {
        if weight < 0.0 {
            return Err("AnimationController.setWeight: negative weight".into());
        }
        self.weight = weight;
        Ok(())
    }

    pub fn is_active(&self, world_time: i32) -> bool {
        if self.active_start == self.active_end {
            true
        } else {
            world_time >= self.active_start && world_time < self.active_end
        }
    }

    pub fn time_to_activation(&self, world_time: i32) -> i32 {
        if world_time < self.active_start {
            self.active_start.wrapping_sub(world_time)
        } else if world_time >= self.active_end {
            i32::MAX
        } else {
            0
        }
    }

    pub fn time_to_deactivation(&self, world_time: i32) -> i32 {
        if world_time < self.active_end {
            self.active_end.wrapping_sub(world_time)
        } else {
            i32::MAX
        }
    }
}

#[derive(Clone, Debug)]
pub struct TrackData {
    pub sequence: Id,
    pub controller: Option<Id>,
    pub property: i32,
}

pub enum Kind {
    Group(GroupData),
    World(WorldData),
    Mesh(MeshData),
    MorphingMesh(MorphingData),
    SkinnedMesh(SkinnedData),
    Sprite3D(SpriteData),
    Camera(CameraData),
    Light(LightData),
    Appearance(AppearanceData),
    Background(BackgroundData),
    CompositingMode(CompositingModeData),
    Fog(FogData),
    PolygonMode(PolygonModeData),
    Material(MaterialData),
    Texture2D(Texture2DData),
    Image2D(Image2DData),
    VertexArray(VertexArrayData),
    VertexBuffer(VertexBufferData),
    TriangleStripArray(TriangleStripData),
    KeyframeSequence(crate::anim::KeyframeSequence),
    AnimationController(ControllerData),
    AnimationTrack(TrackData),
}

impl Kind {
    pub fn name(&self) -> &'static str {
        match self {
            Kind::Group(_) => "Group",
            Kind::World(_) => "World",
            Kind::Mesh(_) => "Mesh",
            Kind::MorphingMesh(_) => "MorphingMesh",
            Kind::SkinnedMesh(_) => "SkinnedMesh",
            Kind::Sprite3D(_) => "Sprite3D",
            Kind::Camera(_) => "Camera",
            Kind::Light(_) => "Light",
            Kind::Appearance(_) => "Appearance",
            Kind::Background(_) => "Background",
            Kind::CompositingMode(_) => "CompositingMode",
            Kind::Fog(_) => "Fog",
            Kind::PolygonMode(_) => "PolygonMode",
            Kind::Material(_) => "Material",
            Kind::Texture2D(_) => "Texture2D",
            Kind::Image2D(_) => "Image2D",
            Kind::VertexArray(_) => "VertexArray",
            Kind::VertexBuffer(_) => "VertexBuffer",
            Kind::TriangleStripArray(_) => "TriangleStripArray",
            Kind::KeyframeSequence(_) => "KeyframeSequence",
            Kind::AnimationController(_) => "AnimationController",
            Kind::AnimationTrack(_) => "AnimationTrack",
        }
    }
}

pub struct Object {
    pub user_id: i32,
    /// Animation tracks, kept sorted by target property.
    pub tracks: Vec<Id>,
    pub kind: Kind,
}

#[derive(Default)]
pub struct Scene {
    pub objs: Vec<Object>,
}

impl Scene {
    pub fn add(&mut self, kind: Kind) -> Id {
        self.objs.push(Object {
            user_id: 0,
            tracks: Vec::new(),
            kind,
        });
        self.objs.len() - 1
    }

    pub fn kind(&self, id: Id) -> &Kind {
        &self.objs[id].kind
    }

    pub fn kind_mut(&mut self, id: Id) -> &mut Kind {
        &mut self.objs[id].kind
    }

    pub fn is_node(&self, id: Id) -> bool {
        self.node_base(id).is_some()
    }

    pub fn node_base(&self, id: Id) -> Option<&NodeBase> {
        match &self.objs[id].kind {
            Kind::Group(g) => Some(&g.node),
            Kind::World(w) => Some(&w.group.node),
            Kind::Mesh(m) => Some(&m.node),
            Kind::MorphingMesh(m) => Some(&m.mesh.node),
            Kind::SkinnedMesh(m) => Some(&m.mesh.node),
            Kind::Sprite3D(s) => Some(&s.node),
            Kind::Camera(c) => Some(&c.node),
            Kind::Light(l) => Some(&l.node),
            _ => None,
        }
    }

    pub fn node_base_mut(&mut self, id: Id) -> Option<&mut NodeBase> {
        match &mut self.objs[id].kind {
            Kind::Group(g) => Some(&mut g.node),
            Kind::World(w) => Some(&mut w.group.node),
            Kind::Mesh(m) => Some(&mut m.node),
            Kind::MorphingMesh(m) => Some(&mut m.mesh.node),
            Kind::SkinnedMesh(m) => Some(&mut m.mesh.node),
            Kind::Sprite3D(s) => Some(&mut s.node),
            Kind::Camera(c) => Some(&mut c.node),
            Kind::Light(l) => Some(&mut l.node),
            _ => None,
        }
    }

    pub fn group(&self, id: Id) -> Option<&GroupData> {
        match &self.objs[id].kind {
            Kind::Group(g) => Some(g),
            Kind::World(w) => Some(&w.group),
            _ => None,
        }
    }

    pub fn group_mut(&mut self, id: Id) -> Option<&mut GroupData> {
        match &mut self.objs[id].kind {
            Kind::Group(g) => Some(g),
            Kind::World(w) => Some(&mut w.group),
            _ => None,
        }
    }

    /// The mesh part of any mesh-like node.
    pub fn mesh(&self, id: Id) -> Option<&MeshData> {
        match &self.objs[id].kind {
            Kind::Mesh(m) => Some(m),
            Kind::MorphingMesh(m) => Some(&m.mesh),
            Kind::SkinnedMesh(m) => Some(&m.mesh),
            _ => None,
        }
    }

    pub fn transformable(&self, id: Id) -> Option<&Transformable> {
        if let Kind::Texture2D(t) = &self.objs[id].kind {
            return Some(&t.tr);
        }
        self.node_base(id).map(|n| &n.tr)
    }

    pub fn parent(&self, id: Id) -> Option<Id> {
        self.node_base(id).and_then(|n| n.parent)
    }

    pub fn composite_transform(&self, id: Id) -> Mat4 {
        self.transformable(id)
            .map(|t| t.composite())
            .unwrap_or_default()
    }

    pub fn root_of(&self, id: Id) -> Id {
        let mut node = id;
        while let Some(p) = self.parent(node) {
            node = p;
        }
        node
    }

    fn depth(&self, id: Id) -> usize {
        let mut d = 0;
        let mut node = id;
        while let Some(p) = self.parent(node) {
            node = p;
            d += 1;
        }
        d
    }

    /// True when `ancestor` is `node` or one of its ancestors (`isParentOf`).
    fn is_parent_of(&self, ancestor: Id, node: Id) -> bool {
        let mut next = Some(node);
        while let Some(cur) = next {
            if cur == ancestor {
                return true;
            }
            next = self.parent(cur);
        }
        false
    }

    pub fn is_descendant_of(&self, node: Id, ancestor: Id) -> bool {
        let mut next = self.parent(node);
        while let Some(cur) = next {
            if cur == ancestor {
                return true;
            }
            next = self.parent(cur);
        }
        false
    }

    /// `Node.getTransformTo`: the transform that maps `from`'s space into `target`'s.
    pub fn get_transform_to(&self, from: Id, target: Id) -> Option<Mat4> {
        if self.root_of(from) != self.root_of(target) {
            return None;
        }
        let mut trans_this = Mat4::IDENTITY;
        let mut trans_target = Mat4::IDENTITY;
        let mut root_this = from;
        let mut root_target = target;
        let mut depth_this = self.depth(from);
        let mut depth_target = self.depth(target);

        while depth_this > depth_target {
            let t = self.composite_transform(root_this);
            trans_this.post_multiply(&t, true);
            depth_this -= 1;
            root_this = self.parent(root_this).expect("depth says a parent exists");
        }
        while depth_target > depth_this {
            let t = self.composite_transform(root_target);
            trans_target.post_multiply(&t, true);
            depth_target -= 1;
            root_target = self
                .parent(root_target)
                .expect("depth says a parent exists");
        }
        while root_this != root_target {
            let t = self.composite_transform(root_this);
            trans_this.post_multiply(&t, true);
            root_this = self
                .parent(root_this)
                .expect("same root, so a parent exists");
            let t = self.composite_transform(root_target);
            trans_target.post_multiply(&t, true);
            root_target = self
                .parent(root_target)
                .expect("same root, so a parent exists");
        }

        trans_target.invert();
        trans_target.post_multiply(&trans_this, false);
        Some(trans_target)
    }

    pub fn add_child(&mut self, parent: Id, child: Id) -> Result<()> {
        if child == parent {
            return Err("child is this Group".into());
        }
        if matches!(self.objs[child].kind, Kind::World(_)) {
            return Err("child is a World node".into());
        }
        let child_parent = self
            .node_base(child)
            .ok_or_else(|| "child is not a Node".to_string())?
            .parent;
        if let Some(p) = child_parent {
            if p != parent {
                return Err("child already has a parent other than this Group".into());
            }
        }
        if self.is_parent_of(child, parent) {
            return Err("child is an ancestor of this Group".into());
        }
        let g = self
            .group_mut(parent)
            .ok_or_else(|| "parent is not a Group".to_string())?;
        if !g.children.contains(&child) {
            g.children.push(child);
            self.node_base_mut(child).expect("checked above").parent = Some(parent);
        }
        Ok(())
    }

    /// `Object3D.addAnimationTrack`.
    pub fn add_animation_track(&mut self, object: Id, track: Id) -> Result<()> {
        let (property, components) = match &self.objs[track].kind {
            Kind::AnimationTrack(t) => {
                let comps = match &self.objs[t.sequence].kind {
                    Kind::KeyframeSequence(k) => k.component_count,
                    _ => return Err("track sequence is not a KeyframeSequence".into()),
                };
                (t.property, comps)
            }
            _ => return Err("not an AnimationTrack".into()),
        };
        if self.objs[object].tracks.contains(&track) || !self.track_compatible(property, object) {
            return Err(format!(
                "AnimationTrack (property {property}) is not compatible with {}",
                self.objs[object].kind.name()
            ));
        }
        let existing = self.objs[object].tracks.clone();
        for (i, &e) in existing.iter().enumerate() {
            let (e_prop, e_comps) = match &self.objs[e].kind {
                Kind::AnimationTrack(t) => (
                    t.property,
                    match &self.objs[t.sequence].kind {
                        Kind::KeyframeSequence(k) => k.component_count,
                        _ => 0,
                    },
                ),
                _ => unreachable!("tracks only hold AnimationTrack ids"),
            };
            if e_prop > property {
                self.objs[object].tracks.insert(i, track);
                return Ok(());
            }
            if e_prop == property && e_comps != components {
                return Err("tracks with the same property differ in component count".into());
            }
        }
        self.objs[object].tracks.push(track);
        Ok(())
    }

    fn track_compatible(&self, property: i32, o: Id) -> bool {
        let k = &self.objs[o].kind;
        let is_node = self.is_node(o);
        let is_transformable = is_node || matches!(k, Kind::Texture2D(_));
        match property {
            prop::ALPHA => {
                is_node
                    || matches!(
                        k,
                        Kind::Background(_) | Kind::Material(_) | Kind::VertexBuffer(_)
                    )
            }
            prop::AMBIENT_COLOR
            | prop::DIFFUSE_COLOR
            | prop::EMISSIVE_COLOR
            | prop::SHININESS
            | prop::SPECULAR_COLOR => matches!(k, Kind::Material(_)),
            prop::COLOR => matches!(
                k,
                Kind::Light(_)
                    | Kind::Background(_)
                    | Kind::Fog(_)
                    | Kind::Texture2D(_)
                    | Kind::VertexBuffer(_)
            ),
            prop::CROP => matches!(k, Kind::Sprite3D(_) | Kind::Background(_)),
            prop::DENSITY => matches!(k, Kind::Fog(_)),
            prop::FAR_DISTANCE | prop::NEAR_DISTANCE => matches!(k, Kind::Camera(_) | Kind::Fog(_)),
            prop::FIELD_OF_VIEW => matches!(k, Kind::Camera(_)),
            prop::INTENSITY | prop::SPOT_ANGLE | prop::SPOT_EXPONENT => matches!(k, Kind::Light(_)),
            prop::MORPH_WEIGHTS => matches!(k, Kind::MorphingMesh(_)),
            prop::ORIENTATION | prop::SCALE | prop::TRANSLATION => is_transformable,
            prop::PICKABILITY | prop::VISIBILITY => is_node,
            _ => false,
        }
    }

    /// Everything `Object3D.references` would hold, which is what `animate` walks.
    pub fn references(&self, id: Id) -> Vec<Id> {
        let mut r: Vec<Id> = self.objs[id].tracks.clone();
        let mesh_refs = |m: &MeshData, r: &mut Vec<Id>| {
            r.push(m.vertices);
            r.extend(m.submeshes.iter().copied());
            r.extend(m.appearances.iter().flatten().copied());
        };
        match &self.objs[id].kind {
            Kind::Group(g) => r.extend(g.children.iter().copied()),
            Kind::World(w) => {
                r.extend(w.group.children.iter().copied());
                r.extend(w.active_camera);
                r.extend(w.background);
            }
            Kind::Mesh(m) => mesh_refs(m, &mut r),
            Kind::MorphingMesh(m) => {
                mesh_refs(&m.mesh, &mut r);
                r.extend(m.targets.iter().copied());
            }
            Kind::SkinnedMesh(m) => {
                mesh_refs(&m.mesh, &mut r);
                r.push(m.skeleton);
            }
            Kind::Sprite3D(s) => {
                r.push(s.image);
                r.extend(s.appearance);
            }
            Kind::Appearance(a) => {
                r.extend(a.compositing_mode);
                r.extend(a.fog);
                r.extend(a.polygon_mode);
                r.extend(a.material);
                r.extend(a.textures.iter().flatten().copied());
            }
            Kind::Background(b) => r.extend(b.image),
            Kind::Texture2D(t) => r.push(t.image),
            Kind::VertexBuffer(v) => {
                r.extend(v.positions);
                r.extend(v.normals);
                r.extend(v.colors);
                r.extend(v.tex_coords.iter().flatten().copied());
            }
            Kind::AnimationTrack(t) => {
                r.push(t.sequence);
                r.extend(t.controller);
            }
            _ => {}
        }
        r
    }
}
