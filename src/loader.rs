//! `.m3g` file loader (JSR-184 file format), a port of `m3g.loader.M3GLoader`.
//!
//! The port keeps the original's behaviour where it is observable in a render:
//! object orientation goes through the same angle/axis round trip, references
//! resolve by the same indices, and unreferenced objects are returned as roots
//! in file order.

use std::path::PathBuf;

use crate::anim::KeyframeSequence;
use crate::math::Mat4;
use crate::scene::*;

const M3G_MAGIC: [u8; 12] = [0xAB, 0x4A, 0x53, 0x52, 0x31, 0x38, 0x34, 0xBB, 0x0D, 0x0A, 0x1A, 0x0A];
/// Mutable images are filled with 0xFF on load, so their size comes from the header alone.
const MAX_MUTABLE_IMAGE_BYTES: usize = 1 << 28;
const PNG_MAGIC: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];

/// Loads a file and returns its root objects (those no other object references).
pub fn load(scene: &mut Scene, path: &str) -> Result<Vec<Id>> {
    let mut history = Vec::new();
    load_named(scene, path, None, &mut history)
}

fn load_named(
    scene: &mut Scene,
    name: &str,
    parent: Option<&str>,
    history: &mut Vec<String>,
) -> Result<Vec<Id>> {
    if history.iter().any(|h| h == name) {
        return Err("Reference loop detected.".into());
    }
    history.push(name.to_string());
    let target = resolve_path(name, parent)?;
    let data = std::fs::read(&target).map_err(|e| format!("{}: {e}", target.display()))?;
    let roots = if data.len() >= 8 && data[..8] == PNG_MAGIC {
        vec![load_png(scene, &data)?]
    } else if data.len() >= 12 && data[..12] == M3G_MAGIC {
        let mut l = Loader {
            scene,
            loaded: Vec::new(),
            history,
            resource_name: name.to_string(),
            current_section: 0,
            contained_external_links: false,
            external_links: false,
        };
        l.load_m3g(&data)?
    } else {
        return Err("File not recognized.".into());
    };
    history.pop();
    Ok(roots)
}

/// External references resolve against the directory of the referencing file's
/// *name* (not its resolved path), exactly as the original does.
fn resolve_path(name: &str, parent: Option<&str>) -> Result<PathBuf> {
    if name.contains(':') {
        return Err(format!("HTTP external references are not supported: {name}"));
    }
    let mut target = PathBuf::from(name);
    if !target.is_absolute() {
        if let Some(parent) = parent {
            let dir = match parent.rfind('/') {
                Some(slash) if slash > 0 => PathBuf::from(&parent[..slash]),
                _ => PathBuf::from("."),
            };
            target = dir.join(name);
        }
    }
    if !target.is_file() {
        return Err(format!("File not found: {}", target.display()));
    }
    Ok(target)
}

fn load_png(scene: &mut Scene, data: &[u8]) -> Result<Id> {
    let mut decoder = png::Decoder::new(data);
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|e| format!("Unsupported or corrupt image data: {e}"))?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).map_err(|e| format!("Unsupported or corrupt image data: {e}"))?;
    let (w, h) = (info.width as usize, info.height as usize);
    let mut rgb = Vec::with_capacity(w * h * 3);
    let channels = info.color_type.samples();
    for px in buf[..info.buffer_size()].chunks(channels) {
        match info.color_type {
            png::ColorType::Rgb | png::ColorType::Rgba => rgb.extend_from_slice(&px[..3]),
            png::ColorType::Grayscale | png::ColorType::GrayscaleAlpha => rgb.extend_from_slice(&[px[0]; 3]),
            png::ColorType::Indexed => return Err("indexed PNG was not expanded".into()),
        }
    }
    Ok(scene.add(Kind::Image2D(Image2DData {
        format: IMAGE_RGB,
        width: w as i32,
        height: h as i32,
        data: rgb,
    })))
}

struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(data: &'a [u8]) -> Self {
        Cursor { data, pos: 0 }
    }

    fn byte(&mut self) -> Result<i32> {
        let b = *self.data.get(self.pos).ok_or("Unexpected end of data")?;
        self.pos += 1;
        Ok(b as i32)
    }

    fn bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(n).ok_or("length overflow")?;
        if end > self.data.len() {
            return Err("Unexpected end of data".into());
        }
        let s = &self.data[self.pos..end];
        self.pos = end;
        Ok(s)
    }

    fn bit(&mut self) -> Result<bool> {
        match self.byte()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err("Malformed boolean.".into()),
        }
    }

    fn i16(&mut self) -> Result<i32> {
        let a = self.byte()?;
        let b = self.byte()?;
        Ok(a + (b << 8))
    }

    fn i32(&mut self) -> Result<i32> {
        let b = self.bytes(4)?;
        Ok(i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn u32(&mut self) -> Result<u32> {
        Ok(self.i32()? as u32)
    }

    fn f32(&mut self) -> Result<f32> {
        let bits = self.i32()?;
        let exp = bits & 0x7f80_0000;
        let ok = exp != 0x7f80_0000
            && bits != i32::MIN
            && ((bits & 0x007f_ffff) == 0 || exp != 0);
        if ok {
            Ok(f32::from_bits(bits as u32))
        } else {
            Err("Malformed float.".into())
        }
    }

    fn rgba(&mut self) -> Result<i32> {
        let r = self.byte()?;
        let g = self.byte()?;
        let b = self.byte()?;
        let a = self.byte()?;
        Ok((r << 16) + (g << 8) + b + (a << 24))
    }

    fn rgb(&mut self) -> Result<i32> {
        let r = self.byte()?;
        let g = self.byte()?;
        let b = self.byte()?;
        Ok((r << 16) + (g << 8) + b)
    }

    /// Modified UTF-8, NUL terminated.
    fn string(&mut self) -> Result<String> {
        let mut units: Vec<u16> = Vec::new();
        loop {
            let first = self.byte()?;
            if first == 0 {
                break;
            }
            if first & 128 == 0 {
                units.push((first & 255) as u16);
            } else if first & 224 == 192 {
                let second = self.byte()?;
                if second & 192 != 128 {
                    return Err("Invalid UTF-8 string.".into());
                }
                units.push((((first & 31) << 6) | (second & 63)) as u16);
            } else if first & 240 == 224 {
                let second = self.byte()?;
                let third = self.byte()?;
                if second & 192 != 128 || third & 192 != 128 {
                    return Err("Invalid UTF-8 string.".into());
                }
                units.push((((first & 15) << 12) | ((second & 63) << 6) | (third & 63)) as u16);
            } else {
                return Err("Invalid UTF-8 string.".into());
            }
        }
        Ok(String::from_utf16_lossy(&units))
    }

    fn transform(&mut self) -> Result<Mat4> {
        let mut m = [0.0f32; 16];
        for v in m.iter_mut() {
            *v = self.f32()?;
        }
        Ok(Mat4::from_array(m))
    }
}

fn adler32(data: &[u8]) -> u32 {
    let (mut s1, mut s2) = (1u32, 0u32);
    for &b in data {
        s1 = (s1 + b as u32) % 65521;
        s2 = (s2 + s1) % 65521;
    }
    (s2 << 16) + s1
}

/// What `readObject3DData` yields: the user id and the animation tracks to attach.
struct Object3DData {
    user_id: i32,
    tracks: Vec<Id>,
}

struct Loader<'a> {
    scene: &'a mut Scene,
    /// Objects in load order with a "has been referenced" flag.
    loaded: Vec<(Id, bool)>,
    history: &'a mut Vec<String>,
    resource_name: String,
    current_section: i32,
    contained_external_links: bool,
    external_links: bool,
}

impl<'a> Loader<'a> {
    fn load_m3g(&mut self, data: &[u8]) -> Result<Vec<Id>> {
        let mut pos = M3G_MAGIC.len();
        while pos < data.len() {
            match self.load_section(data, &mut pos)? {
                true => self.current_section += 1,
                false => break,
            }
        }
        Ok(self.loaded.iter().filter(|(_, referenced)| !referenced).map(|(id, _)| *id).collect())
    }

    /// Returns false when loading should stop quietly (the original logs and stops).
    fn load_section(&mut self, data: &[u8], pos: &mut usize) -> Result<bool> {
        if self.current_section > 1 && self.external_links && !self.contained_external_links {
            return Err(format!("No external sections ({}).", self.resource_name));
        }
        let start = *pos;
        let mut c = Cursor { data, pos: start };
        let compression = c.byte()?;
        if self.current_section == 0 && compression != 0 {
            return Err(format!("Compressed header ({}).", self.resource_name));
        }
        let section_length = c.u32()? as usize;
        let uncompressed_length = c.u32()? as usize;

        if section_length < 13 || start + section_length > data.len() {
            return Err("Section length exceeds the file.".into());
        }
        let payload: Vec<u8>;
        let body: &[u8] = match compression {
            0 => {
                if uncompressed_length != section_length - 13 {
                    eprintln!("[m3g-viewer] ERROR M3GLoader: Section length mismatch!!!");
                    return Ok(false);
                }
                &data[start + 9..start + 9 + uncompressed_length]
            }
            1 => {
                let compressed = &data[start + 9..start + section_length - 4];
                payload = miniz_oxide::inflate::decompress_to_vec_zlib(compressed)
                    .map_err(|e| format!("m3g unzip error: {e:?}"))?;
                if payload.len() != uncompressed_length {
                    return Err("Inflated section has the wrong length.".into());
                }
                &payload
            }
            other => {
                eprintln!("[m3g-viewer] ERROR M3GLoader: Unrecognized compression scheme({other})!!!");
                return Ok(false);
            }
        };

        let checksum = adler32(&data[start..start + section_length - 4]);
        let mut tail = Cursor { data, pos: start + section_length - 4 };
        let expected = tail.u32()?;

        let mut objects = Cursor::new(body);
        while objects.pos < body.len() {
            if let Some(id) = self.load_object(&mut objects)? {
                self.loaded.push((id, false));
            }
        }

        if checksum != expected {
            return Err(format!("Checksum is wrong ({}).", self.resource_name));
        }
        *pos = start + section_length;
        Ok(true)
    }

    fn load_object(&mut self, section: &mut Cursor) -> Result<Option<Id>> {
        let object_type = section.byte()?;
        let length = section.u32()? as usize;
        let body = section.bytes(length)?;
        let mut r = Cursor::new(body);

        let id = match object_type {
            0 => {
                if self.current_section != 0 {
                    return Err(format!("Header in wrong section ({}).", self.resource_name));
                }
                self.read_header(&mut r)?;
                None
            }
            1 => Some(self.read_animation_controller(&mut r)?),
            2 => Some(self.read_animation_track(&mut r)?),
            3 => Some(self.read_appearance(&mut r)?),
            4 => Some(self.read_background(&mut r)?),
            5 => Some(self.read_camera(&mut r)?),
            6 => Some(self.read_compositing_mode(&mut r)?),
            7 => Some(self.read_fog(&mut r)?),
            8 => Some(self.read_polygon_mode(&mut r)?),
            9 => Some(self.read_group(&mut r)?),
            10 => Some(self.read_image2d(&mut r)?),
            11 => Some(self.read_triangle_strip_array(&mut r)?),
            12 => Some(self.read_light(&mut r)?),
            13 => Some(self.read_material(&mut r)?),
            14 => Some(self.read_mesh(&mut r)?),
            15 => Some(self.read_morphing_mesh(&mut r)?),
            16 => Some(self.read_skinned_mesh(&mut r)?),
            17 => Some(self.read_texture2d(&mut r)?),
            18 => Some(self.read_sprite(&mut r)?),
            19 => Some(self.read_keyframe_sequence(&mut r)?),
            20 => Some(self.read_vertex_array(&mut r)?),
            21 => Some(self.read_vertex_buffer(&mut r)?),
            22 => Some(self.read_world(&mut r)?),
            255 => {
                if self.current_section != 1 {
                    return Err(format!(
                        "External reference in wrong section ({}).",
                        self.resource_name
                    ));
                }
                if !self.external_links {
                    return Err(format!(
                        "External links in self contained file ({}).",
                        self.resource_name
                    ));
                }
                let uri = r.string()?;
                self.contained_external_links = true;
                let name = self.resource_name.clone();
                let roots = load_named(self.scene, &uri, Some(&name), self.history)?;
                Some(*roots.first().ok_or("external reference holds no objects")?)
            }
            other => {
                return Err(format!("Unrecognized object type {other} ({}).", self.resource_name));
            }
        };

        if r.pos != body.len() {
            return Err(format!("Object length mismatch ({}).", self.resource_name));
        }
        Ok(id)
    }

    // ----------------------------------------------------------- references

    fn get_loaded(&mut self, index: u32) -> Result<Option<Id>> {
        if index == 0 {
            Ok(None)
        } else if index >= 2 && ((index - 2) as usize) < self.loaded.len() {
            let entry = &mut self.loaded[(index - 2) as usize];
            entry.1 = true;
            Ok(Some(entry.0))
        } else {
            Err(format!("Invalid reference index ({}).", self.resource_name))
        }
    }

    fn reference(&mut self, r: &mut Cursor) -> Result<Option<Id>> {
        let index = r.u32()?;
        self.get_loaded(index)
    }

    /// A reference that must be present and of one kind.
    fn reference_as(
        &mut self,
        r: &mut Cursor,
        what: &str,
        check: fn(&Kind) -> bool,
    ) -> Result<Id> {
        self.reference_as_opt(r, what, check)?
            .ok_or_else(|| format!("Null {what} reference"))
    }

    fn reference_as_opt(
        &mut self,
        r: &mut Cursor,
        what: &str,
        check: fn(&Kind) -> bool,
    ) -> Result<Option<Id>> {
        match self.reference(r)? {
            None => Ok(None),
            Some(id) if check(self.scene.kind(id)) => Ok(Some(id)),
            Some(id) => Err(format!(
                "Expected a {what} but the file references a {}",
                self.scene.kind(id).name()
            )),
        }
    }

    // --------------------------------------------------------- object data

    fn finish(&mut self, id: Id, data: Object3DData) -> Result<Id> {
        self.scene.objs[id].user_id = data.user_id;
        for track in data.tracks {
            self.scene.add_animation_track(id, track)?;
        }
        Ok(id)
    }

    fn read_object3d_data(&mut self, r: &mut Cursor) -> Result<Object3DData> {
        let user_id = r.u32()? as i32;
        let track_count = r.u32()?;
        let mut tracks = Vec::new();
        for _ in 0..track_count {
            let t = self
                .reference_as(r, "AnimationTrack", |k| matches!(k, Kind::AnimationTrack(_)))?;
            tracks.push(t);
        }
        let parameter_count = r.u32()?;
        for _ in 0..parameter_count {
            let _id = r.u32()?;
            let len = r.u32()? as usize;
            r.bytes(len)?;
        }
        Ok(Object3DData { user_id, tracks })
    }

    fn read_transformable_data(&mut self, r: &mut Cursor) -> Result<(Object3DData, Transformable)> {
        let data = self.read_object3d_data(r)?;
        let mut tr = Transformable::default();
        if r.bit()? {
            let t = [r.f32()?, r.f32()?, r.f32()?];
            tr.translation = t;
            let s = [r.f32()?, r.f32()?, r.f32()?];
            tr.scale = s;
            let (a, x, y, z) = (r.f32()?, r.f32()?, r.f32()?, r.f32()?);
            tr.set_orientation(a, x, y, z)?;
        }
        if r.bit()? {
            tr.transform = r.transform()?;
        }
        Ok((data, tr))
    }

    fn read_node_data(&mut self, r: &mut Cursor) -> Result<(Object3DData, NodeBase)> {
        let (data, tr) = self.read_transformable_data(r)?;
        let mut node = NodeBase { tr, ..NodeBase::default() };
        node.rendering = r.bit()?;
        node.picking = r.bit()?;
        node.alpha_factor = (r.byte()? as f32) / 255.0;
        node.scope = r.u32()? as i32;
        if r.bit()? {
            // Alignment targets and references only matter to Node.align(),
            // which the viewer never calls.
            let _z_target = r.byte()?;
            let _y_target = r.byte()?;
            let z_ref = r.u32()?;
            let y_ref = r.u32()?;
            self.get_loaded(z_ref)?;
            self.get_loaded(y_ref)?;
        }
        Ok((data, node))
    }

    fn read_group_data(&mut self, r: &mut Cursor) -> Result<(Object3DData, GroupData)> {
        let (data, node) = self.read_node_data(r)?;
        let child_count = r.u32()?;
        let mut children = Vec::new();
        for _ in 0..child_count {
            let id = self.reference_as(r, "Node", |k| {
                matches!(
                    k,
                    Kind::Group(_)
                        | Kind::World(_)
                        | Kind::Mesh(_)
                        | Kind::MorphingMesh(_)
                        | Kind::SkinnedMesh(_)
                        | Kind::Sprite3D(_)
                        | Kind::Camera(_)
                        | Kind::Light(_)
                )
            })?;
            children.push(id);
        }
        Ok((data, GroupData { node, children }))
    }

    /// `copyTransformable`: the original rebuilds orientation through get/setOrientation.
    fn copy_transformable(src: &mut Transformable) -> Result<Transformable> {
        let mut t = Transformable::default();
        t.translation = src.translation;
        t.scale = src.scale;
        let a = src.get_orientation();
        t.set_orientation(a[0], a[1], a[2], a[3])?;
        t.transform = src.transform;
        Ok(t)
    }

    /// `copyNode`.
    fn copy_node(src: &mut NodeBase) -> Result<NodeBase> {
        let mut n = NodeBase::default();
        n.tr = Self::copy_transformable(&mut src.tr)?;
        n.alpha_factor = src.alpha_factor;
        n.scope = src.scope;
        n.picking = src.picking;
        n.rendering = src.rendering;
        Ok(n)
    }

    /// Adds a group's children to the arena and fixes their parent links.
    fn attach_children(&mut self, parent: Id, children: &[Id]) -> Result<()> {
        // `read_group_data` collected children without linking them, so that
        // `Scene::add_child` can run its validation once the parent has an id.
        for &c in children {
            self.scene.add_child(parent, c)?;
        }
        Ok(())
    }

    // ------------------------------------------------------------- readers

    fn read_header(&mut self, r: &mut Cursor) -> Result<()> {
        let v0 = r.byte()?;
        let v1 = r.byte()?;
        self.external_links = r.bit()?;
        r.u32()?;
        r.u32()?;
        if v0 != 1 || v1 != 0 {
            return Err(format!("Invalid file version ({}).", self.resource_name));
        }
        r.string()?;
        Ok(())
    }

    fn read_animation_controller(&mut self, r: &mut Cursor) -> Result<Id> {
        let data = self.read_object3d_data(r)?;
        let mut c = ControllerData::default();
        let speed = r.f32()?;
        let weight = r.f32()?;
        let (start, end) = (r.i32()?, r.i32()?);
        c.set_active_interval(start, end)?;
        let sequence_time = r.f32()?;
        let reference_world_time = r.i32()?;
        c.set_position(sequence_time, reference_world_time);
        c.set_speed(speed, reference_world_time);
        c.set_weight(weight)?;
        let id = self.scene.add(Kind::AnimationController(c));
        self.finish(id, data)
    }

    fn read_animation_track(&mut self, r: &mut Cursor) -> Result<Id> {
        let data = self.read_object3d_data(r)?;
        let sequence =
            self.reference_as(r, "KeyframeSequence", |k| matches!(k, Kind::KeyframeSequence(_)))?;
        let controller = self.reference_as_opt(r, "AnimationController", |k| {
            matches!(k, Kind::AnimationController(_))
        })?;
        let property = r.u32()? as i32;
        let components = match self.scene.kind(sequence) {
            Kind::KeyframeSequence(k) => k.component_count,
            _ => unreachable!("checked above"),
        };
        if !valid_component_count(property, components) {
            return Err(format!(
                "AnimationTrack: property {property} cannot take {components} components"
            ));
        }
        let id = self.scene.add(Kind::AnimationTrack(TrackData { sequence, controller, property }));
        self.finish(id, data)
    }

    fn read_appearance(&mut self, r: &mut Cursor) -> Result<Id> {
        let data = self.read_object3d_data(r)?;
        let mut a = AppearanceData::default();
        let layer = r.byte()?;
        if !(-63..=63).contains(&layer) {
            return Err("Appearance.setLayer: layer out of range".into());
        }
        a.layer = layer;
        a.compositing_mode = self
            .reference_as_opt(r, "CompositingMode", |k| matches!(k, Kind::CompositingMode(_)))?;
        a.fog = self.reference_as_opt(r, "Fog", |k| matches!(k, Kind::Fog(_)))?;
        a.polygon_mode =
            self.reference_as_opt(r, "PolygonMode", |k| matches!(k, Kind::PolygonMode(_)))?;
        a.material = self.reference_as_opt(r, "Material", |k| matches!(k, Kind::Material(_)))?;
        let texture_count = r.u32()? as usize;
        for i in 0..texture_count {
            let t = self.reference_as(r, "Texture2D", |k| matches!(k, Kind::Texture2D(_)))?;
            if i >= MAX_TEXTURE_UNITS {
                return Err("Appearance.setTexture: index out of bounds".into());
            }
            a.textures[i] = Some(t);
        }
        let id = self.scene.add(Kind::Appearance(a));
        self.finish(id, data)
    }

    fn read_background(&mut self, r: &mut Cursor) -> Result<Id> {
        let data = self.read_object3d_data(r)?;
        let mut b = BackgroundData::default();
        b.color = r.rgba()?;
        let image = self.reference_as_opt(r, "Image2D", |k| matches!(k, Kind::Image2D(_)))?;
        if let Some(img) = image {
            let (w, h, f) = match self.scene.kind(img) {
                Kind::Image2D(i) => (i.width, i.height, i.format),
                _ => unreachable!(),
            };
            if f != IMAGE_RGB && f != IMAGE_RGBA {
                return Err("Background.setImage: image must be RGB or RGBA".into());
            }
            b.image = Some(img);
            b.crop = [0, 0, w, h];
        }
        let (mx, my) = (r.byte()?, r.byte()?);
        if (mx != 32 && mx != 33) || (my != 32 && my != 33) {
            return Err("Background.setImageMode: invalid mode".into());
        }
        b.mode_x = mx;
        b.mode_y = my;
        let crop = [r.i32()?, r.i32()?, r.i32()?, r.i32()?];
        if crop[2] < 0 || crop[3] < 0 {
            return Err("Background.setCrop: negative size".into());
        }
        b.crop = crop;
        b.depth_clear = r.bit()?;
        b.color_clear = r.bit()?;
        let id = self.scene.add(Kind::Background(b));
        self.finish(id, data)
    }

    fn read_camera(&mut self, r: &mut Cursor) -> Result<Id> {
        let (data, node) = self.read_node_data(r)?;
        let mut c = CameraData::new(node);
        match r.byte()? {
            48 => {
                c.generic = r.transform()?;
                c.projection_type = CAMERA_GENERIC;
            }
            50 => {
                let (a, b, cc, d) = (r.f32()?, r.f32()?, r.f32()?, r.f32()?);
                c.set_perspective(a, b, cc, d)?;
            }
            49 => {
                let (a, b, cc, d) = (r.f32()?, r.f32()?, r.f32()?, r.f32()?);
                c.set_parallel(a, b, cc, d)?;
            }
            other => {
                return Err(format!(
                    "Projection type not recognized: {other}({}).",
                    self.resource_name
                ));
            }
        }
        let id = self.scene.add(Kind::Camera(c));
        self.finish(id, data)
    }

    fn read_compositing_mode(&mut self, r: &mut Cursor) -> Result<Id> {
        let data = self.read_object3d_data(r)?;
        let mut c = CompositingModeData::default();
        c.depth_test = r.bit()?;
        c.depth_write = r.bit()?;
        c.color_write = r.bit()?;
        c.alpha_write = r.bit()?;
        let blending = r.byte()?;
        if !(64..=68).contains(&blending) {
            return Err("CompositingMode.setBlending: invalid mode".into());
        }
        c.blending = blending;
        let threshold = (r.byte()? as f32) / 255.0;
        c.alpha_threshold = threshold;
        c.depth_offset_factor = r.f32()?;
        c.depth_offset_units = r.f32()?;
        let id = self.scene.add(Kind::CompositingMode(c));
        self.finish(id, data)
    }

    fn read_fog(&mut self, r: &mut Cursor) -> Result<Id> {
        let data = self.read_object3d_data(r)?;
        let mut f = FogData::default();
        f.color = r.rgb()?;
        let mode = r.byte()?;
        if mode != FOG_EXPONENTIAL && mode != FOG_LINEAR {
            return Err("Fog.setMode: invalid mode".into());
        }
        f.mode = mode;
        if mode == FOG_EXPONENTIAL {
            let d = r.f32()?;
            if d < 0.0 {
                return Err("Fog.setDensity: negative density".into());
            }
            f.density = d;
        } else {
            f.near = r.f32()?;
            f.far = r.f32()?;
        }
        let id = self.scene.add(Kind::Fog(f));
        self.finish(id, data)
    }

    fn read_polygon_mode(&mut self, r: &mut Cursor) -> Result<Id> {
        let data = self.read_object3d_data(r)?;
        let mut p = PolygonModeData::default();
        let culling = r.byte()?;
        if !(160..=162).contains(&culling) {
            return Err("PolygonMode.setCulling: invalid mode".into());
        }
        p.culling = culling;
        let shading = r.byte()?;
        if shading != SHADE_FLAT && shading != SHADE_SMOOTH {
            return Err("PolygonMode.setShading: invalid mode".into());
        }
        p.shading = shading;
        let winding = r.byte()?;
        if winding != WINDING_CCW && winding != WINDING_CW {
            return Err("PolygonMode.setWinding: invalid mode".into());
        }
        p.winding = winding;
        p.two_sided_lighting = r.bit()?;
        p.local_camera_lighting = r.bit()?;
        p.perspective_correction = r.bit()?;
        let id = self.scene.add(Kind::PolygonMode(p));
        self.finish(id, data)
    }

    fn read_group(&mut self, r: &mut Cursor) -> Result<Id> {
        let (data, g) = self.read_group_data(r)?;
        let children = g.children.clone();
        let id = self.scene.add(Kind::Group(GroupData { node: g.node, children: Vec::new() }));
        self.attach_children(id, &children)?;
        self.finish(id, data)
    }

    fn read_world(&mut self, r: &mut Cursor) -> Result<Id> {
        let (data, g) = self.read_group_data(r)?;
        let camera = self.reference_as_opt(r, "Camera", |k| matches!(k, Kind::Camera(_)))?;
        let background =
            self.reference_as_opt(r, "Background", |k| matches!(k, Kind::Background(_)))?;
        let children = g.children.clone();
        let id = self.scene.add(Kind::World(WorldData {
            group: GroupData { node: g.node, children: Vec::new() },
            active_camera: camera,
            background,
            world_time: 0,
        }));
        self.attach_children(id, &children)?;
        self.finish(id, data)
    }

    fn read_image2d(&mut self, r: &mut Cursor) -> Result<Id> {
        let data = self.read_object3d_data(r)?;
        let format = r.byte()?;
        let mutable = r.bit()?;
        let width = r.u32()? as i32;
        let height = r.u32()? as i32;
        let bpp = bytes_per_pixel(format)?;
        if width <= 0 || height <= 0 {
            return Err("Image2D: invalid size".into());
        }
        let pixel_count = (width as usize)
            .checked_mul(height as usize)
            .ok_or("Image2D: size overflows")?;
        let byte_count = pixel_count.checked_mul(bpp).ok_or("Image2D: size overflows")?;
        let pixels: Vec<u8> = if mutable {
            if byte_count > MAX_MUTABLE_IMAGE_BYTES {
                return Err(format!("Image2D: mutable image of {byte_count} bytes is too large"));
            }
            vec![0xFF; byte_count]
        } else {
            let palette_len = r.u32()? as usize;
            let palette = if palette_len > 0 { r.bytes(palette_len)?.to_vec() } else { Vec::new() };
            let pixel_len = r.u32()? as usize;
            let raw = r.bytes(pixel_len)?;
            if palette_len != 0 {
                if raw.len() < pixel_count {
                    return Err("Image2D: not enough pixel data".into());
                }
                if palette.len() < 256 * bpp && palette.len() % bpp != 0 {
                    return Err("Image2D: invalid palette".into());
                }
                let mut out = Vec::with_capacity(byte_count);
                for &index in &raw[..pixel_count] {
                    let from = (index as usize) * bpp;
                    let entry = palette
                        .get(from..from + bpp)
                        .ok_or("Image2D: palette index out of range")?;
                    out.extend_from_slice(entry);
                }
                out
            } else {
                if raw.len() < byte_count {
                    return Err("Image2D: not enough pixel data".into());
                }
                raw[..byte_count].to_vec()
            }
        };
        let id = self.scene.add(Kind::Image2D(Image2DData { format, width, height, data: pixels }));
        self.finish(id, data)
    }

    fn read_keyframe_sequence(&mut self, r: &mut Cursor) -> Result<Id> {
        let data = self.read_object3d_data(r)?;
        let interpolation = r.byte()?;
        let repeat_mode = r.byte()?;
        let encoding = r.byte()?;
        let duration = r.u32()? as i32;
        let valid_first = r.u32()? as i32;
        let valid_last = r.u32()? as i32;
        let component_count = r.u32()? as usize;
        let keyframe_count = r.u32()? as usize;
        // Every keyframe carries a 4-byte time and at least one byte per component, so
        // counts the rest of the object cannot hold are corrupt (and would allocate wildly).
        let per_key = 4usize.saturating_add(component_count);
        if component_count > r.data.len() - r.pos
            || keyframe_count.checked_mul(per_key).map_or(true, |need| need > r.data.len() - r.pos)
        {
            return Err("KeyframeSequence: counts exceed the object's data".into());
        }
        let mut seq = KeyframeSequence::new(keyframe_count, component_count, interpolation)?;
        seq.set_repeat_mode(repeat_mode)?;
        seq.set_duration(duration);
        seq.set_valid_range(valid_first, valid_last)?;
        let mut value = vec![0.0f32; component_count];
        match encoding {
            0 => {
                for k in 0..keyframe_count {
                    let time = r.i32()?;
                    for v in value.iter_mut() {
                        *v = r.f32()?;
                    }
                    seq.set_keyframe(k, time, &value)?;
                }
            }
            1 | 2 => {
                let mut bias = vec![0.0f32; component_count];
                let mut scale = vec![0.0f32; component_count];
                for b in bias.iter_mut() {
                    *b = r.f32()?;
                }
                for s in scale.iter_mut() {
                    *s = r.f32()?;
                }
                for k in 0..keyframe_count {
                    let time = r.i32()?;
                    for c in 0..component_count {
                        if encoding == 1 {
                            let e = r.byte()?;
                            value[c] = bias[c] + scale[c] * (e as f32) / 255.0;
                        } else {
                            let e = r.i16()?;
                            value[c] = bias[c] + scale[c] * (e as f32) / 65535.0;
                        }
                    }
                    seq.set_keyframe(k, time, &value)?;
                }
            }
            other => {
                return Err(format!("Encoding not recognized: {other}({}).", self.resource_name));
            }
        }
        let id = self.scene.add(Kind::KeyframeSequence(seq));
        self.finish(id, data)
    }

    fn read_light(&mut self, r: &mut Cursor) -> Result<Id> {
        let (data, node) = self.read_node_data(r)?;
        let mut l = LightData::new(node);
        let (c, li, q) = (r.f32()?, r.f32()?, r.f32()?);
        if c < 0.0 || li < 0.0 || q < 0.0 || (c == 0.0 && li == 0.0 && q == 0.0) {
            return Err("Light.setAttenuation: invalid attenuation".into());
        }
        l.constant = c;
        l.linear = li;
        l.quadratic = q;
        l.color = r.rgb()?;
        let mode = r.byte()?;
        if !(128..=131).contains(&mode) {
            return Err("Light.setMode: invalid mode".into());
        }
        l.mode = mode;
        l.intensity = r.f32()?;
        let spot_angle = r.f32()?;
        if !(0.0..=90.0).contains(&spot_angle) {
            return Err("Light.setSpotAngle: angle is not in [0, 90]".into());
        }
        l.spot_angle = spot_angle;
        let spot_exponent = r.f32()?;
        if !(0.0..=128.0).contains(&spot_exponent) {
            return Err("Light.setSpotExponent: exponent is not in [0, 128]".into());
        }
        l.spot_exponent = spot_exponent;
        let id = self.scene.add(Kind::Light(l));
        self.finish(id, data)
    }

    fn read_material(&mut self, r: &mut Cursor) -> Result<Id> {
        let data = self.read_object3d_data(r)?;
        let mut m = MaterialData::default();
        m.ambient = r.rgb()?;
        m.diffuse = r.rgba()?;
        m.emissive = r.rgb()?;
        m.specular = r.rgb()?;
        let shininess = r.f32()?;
        if !(0.0..=128.0).contains(&shininess) {
            return Err("Material.setShininess: out of range".into());
        }
        m.shininess = shininess;
        m.vertex_color_tracking = r.bit()?;
        let id = self.scene.add(Kind::Material(m));
        self.finish(id, data)
    }

    /// Reads the mesh part shared by Mesh, MorphingMesh and SkinnedMesh. The
    /// node data goes through `copyNode`, as in the original.
    fn read_mesh_data(&mut self, r: &mut Cursor) -> Result<(Object3DData, MeshData)> {
        let (data, mut node) = self.read_node_data(r)?;
        let vertices =
            self.reference_as(r, "VertexBuffer", |k| matches!(k, Kind::VertexBuffer(_)))?;
        let count = r.u32()? as usize;
        if count == 0 {
            return Err("Mesh: no submeshes".into());
        }
        let mut submeshes = Vec::new();
        let mut appearances = Vec::new();
        for _ in 0..count {
            submeshes.push(self.reference_as(r, "TriangleStripArray", |k| {
                matches!(k, Kind::TriangleStripArray(_))
            })?);
            appearances
                .push(self.reference_as_opt(r, "Appearance", |k| matches!(k, Kind::Appearance(_)))?);
        }
        let node = Self::copy_node(&mut node)?;
        Ok((data, MeshData { node, vertices, submeshes, appearances }))
    }

    fn read_mesh(&mut self, r: &mut Cursor) -> Result<Id> {
        let (data, mesh) = self.read_mesh_data(r)?;
        let id = self.scene.add(Kind::Mesh(mesh));
        self.finish(id, data)
    }

    fn read_morphing_mesh(&mut self, r: &mut Cursor) -> Result<Id> {
        let (data, mut mesh) = self.read_mesh_data(r)?;
        // MorphingMesh is built from the Mesh and then copyMesh'd again.
        mesh.node = Self::copy_node(&mut mesh.node)?;
        let count = r.u32()? as usize;
        let mut targets = Vec::new();
        let mut weights = Vec::new();
        for _ in 0..count {
            let t = self.reference_as(r, "VertexBuffer", |k| matches!(k, Kind::VertexBuffer(_)))?;
            if let Kind::VertexBuffer(v) = self.scene.kind(t) {
                if v.vertex_count == 0 {
                    return Err("MorphingMesh: targets is empty".into());
                }
            }
            targets.push(t);
            weights.push(r.f32()?);
        }
        let mut base_weight = 1.0f32;
        for w in &weights {
            base_weight -= *w;
        }
        let id = self.scene.add(Kind::MorphingMesh(MorphingData { mesh, targets, weights, base_weight }));
        self.finish(id, data)
    }

    fn read_skinned_mesh(&mut self, r: &mut Cursor) -> Result<Id> {
        let (data, mut mesh) = self.read_mesh_data(r)?;
        mesh.node = Self::copy_node(&mut mesh.node)?;
        let skeleton = self.reference_as(r, "Group", |k| matches!(k, Kind::Group(_)))?;
        if self.scene.parent(skeleton).is_some() {
            return Err("SkinnedMesh: skeleton already has a parent".into());
        }
        let vertex_count = match self.scene.kind(mesh.vertices) {
            Kind::VertexBuffer(v) => v.vertex_count,
            _ => unreachable!("checked in read_mesh_data"),
        };
        let skinned = SkinnedData {
            mesh,
            skeleton,
            bones: Vec::new(),
            vtx_bones: vec![0; vertex_count * MAX_TRANSFORMS_PER_VERTEX],
            vtx_weights: vec![0; vertex_count * MAX_TRANSFORMS_PER_VERTEX],
        };
        let id = self.scene.add(Kind::SkinnedMesh(skinned));
        self.scene.node_base_mut(skeleton).expect("a Group is a node").parent = Some(id);

        let bone_count = r.u32()?;
        for _ in 0..bone_count {
            let bone = self.reference_as(r, "Node", |k| {
                matches!(
                    k,
                    Kind::Group(_)
                        | Kind::Mesh(_)
                        | Kind::MorphingMesh(_)
                        | Kind::SkinnedMesh(_)
                        | Kind::Sprite3D(_)
                        | Kind::Camera(_)
                        | Kind::Light(_)
                )
            })?;
            let first_vertex = r.u32()? as i64;
            let vertex_count = r.u32()? as i64;
            let weight = r.i32()?;
            self.add_bone_transform(id, bone, weight, first_vertex, vertex_count)?;
        }
        self.finish(id, data)
    }

    /// `SkinnedMesh.addTransform`.
    fn add_bone_transform(
        &mut self,
        mesh: Id,
        bone: Id,
        weight: i32,
        first_vertex: i64,
        num_vertices: i64,
    ) -> Result<()> {
        let skeleton = match self.scene.kind(mesh) {
            Kind::SkinnedMesh(s) => s.skeleton,
            _ => unreachable!(),
        };
        if bone != skeleton && !self.scene.is_descendant_of(bone, skeleton) {
            return Err("SkinnedMesh.addTransform: bone is not part of the skeleton".into());
        }
        if weight <= 0 || num_vertices <= 0 {
            return Err("SkinnedMesh.addTransform: invalid weight or vertex count".into());
        }
        if first_vertex < 0 || first_vertex + num_vertices > 65535 {
            return Err("SkinnedMesh.addTransform: vertex range out of bounds".into());
        }

        let existing = match self.scene.kind(mesh) {
            Kind::SkinnedMesh(s) => s.bones.iter().position(|b| b.bone == bone),
            _ => unreachable!(),
        };
        let bone_id = match existing {
            Some(i) => i,
            None => {
                let to_bone = self
                    .scene
                    .get_transform_to(mesh, bone)
                    .ok_or("SkinnedMesh.addTransform: no transform to bone")?;
                match self.scene.kind_mut(mesh) {
                    Kind::SkinnedMesh(s) => {
                        s.bones.push(BoneTransform { bone, to_bone });
                        s.bones.len() - 1
                    }
                    _ => unreachable!(),
                }
            }
        };

        let skinned = match self.scene.kind_mut(mesh) {
            Kind::SkinnedMesh(s) => s,
            _ => unreachable!(),
        };
        let per = MAX_TRANSFORMS_PER_VERTEX;
        for i in (first_vertex as usize)..((first_vertex + num_vertices) as usize) {
            if (i + 1) * per > skinned.vtx_bones.len() {
                return Err("SkinnedMesh.addTransform: vertex index beyond the vertex buffer".into());
            }
            let mut min_weight = i32::MAX;
            let mut sel: i32 = -1;
            for slot in 0..per {
                let slot_weight = skinned.vtx_weights[i * per + slot];
                if skinned.vtx_bones[i * per + slot] == bone_id as i32 + 1 {
                    sel = slot as i32;
                    break;
                }
                if slot_weight <= min_weight {
                    min_weight = slot_weight;
                    sel = slot as i32;
                    if slot_weight == 0 {
                        break;
                    }
                }
            }
            let sel_idx = i * per + sel.max(0) as usize;
            if skinned.vtx_bones[sel_idx] == bone_id as i32 + 1 {
                skinned.vtx_weights[sel_idx] += weight;
            } else {
                if min_weight > weight {
                    sel = -1;
                }
                if sel != -1 {
                    skinned.vtx_bones[sel_idx] = bone_id as i32 + 1;
                    skinned.vtx_weights[sel_idx] = weight;
                }
            }
        }
        Ok(())
    }

    fn read_sprite(&mut self, r: &mut Cursor) -> Result<Id> {
        let (data, mut node) = self.read_node_data(r)?;
        let image = self.reference_as(r, "Image2D", |k| matches!(k, Kind::Image2D(_)))?;
        let appearance =
            self.reference_as_opt(r, "Appearance", |k| matches!(k, Kind::Appearance(_)))?;
        let scaled = r.bit()?;
        let node = Self::copy_node(&mut node)?;
        let crop = [r.i32()?, r.i32()?, r.i32()?, r.i32()?];
        if crop[2].abs() > 1024 || crop[3].abs() > 1024 {
            return Err("Sprite3D.setCrop: width or height exceeds 1024".into());
        }
        let id = self.scene.add(Kind::Sprite3D(SpriteData { node, scaled, image, appearance, crop }));
        self.finish(id, data)
    }

    fn read_texture2d(&mut self, r: &mut Cursor) -> Result<Id> {
        let (data, mut tr) = self.read_transformable_data(r)?;
        let image = self.reference_as(r, "Image2D", |k| matches!(k, Kind::Image2D(_)))?;
        let (w, h) = match self.scene.kind(image) {
            Kind::Image2D(i) => (i.width, i.height),
            _ => unreachable!(),
        };
        let pow2 = |v: i32| v & (v - 1) == 0;
        if !pow2(w) || !pow2(h) {
            return Err("Texture2D: the width or height of image is not a power of two".into());
        }
        if w > 2048 || h > 2048 {
            return Err("Texture2D: image exceeds MaxTextureDimension 2048".into());
        }
        let tr = Self::copy_transformable(&mut tr)?;
        let mut t = Texture2DData {
            tr,
            image,
            wrap_s: WRAP_REPEAT,
            wrap_t: WRAP_REPEAT,
            level_filter: FILTER_BASE_LEVEL,
            image_filter: FILTER_NEAREST,
            blending: FUNC_MODULATE,
            blend_color: 0,
        };
        t.blend_color = r.rgb()?;
        let blending = r.byte()?;
        if !(224..=228).contains(&blending) {
            return Err("Texture2D.setBlending: invalid function".into());
        }
        t.blending = blending;
        let (ws, wt) = (r.byte()?, r.byte()?);
        if (ws != WRAP_CLAMP && ws != WRAP_REPEAT) || (wt != WRAP_CLAMP && wt != WRAP_REPEAT) {
            return Err("Texture2D.setWrapping: invalid mode".into());
        }
        t.wrap_s = ws;
        t.wrap_t = wt;
        let (level, image_filter) = (r.byte()?, r.byte()?);
        if !(208..=210).contains(&level) || (image_filter != 209 && image_filter != 210) {
            return Err("Texture2D.setFiltering: invalid filter".into());
        }
        t.level_filter = level;
        t.image_filter = image_filter;
        let id = self.scene.add(Kind::Texture2D(t));
        self.finish(id, data)
    }

    fn read_triangle_strip_array(&mut self, r: &mut Cursor) -> Result<Id> {
        let data = self.read_object3d_data(r)?;
        let encoding = r.byte()?;
        let mut first_index = 0i32;
        let mut indices: Option<Vec<i32>> = None;
        match encoding {
            0 => first_index = r.u32()? as i32,
            1 => first_index = r.byte()?,
            2 => first_index = r.i16()?,
            128 | 129 | 130 => {
                let n = r.u32()? as usize;
                let mut v = Vec::new();
                for _ in 0..n {
                    v.push(match encoding {
                        128 => r.u32()? as i32,
                        129 => r.byte()?,
                        _ => r.i16()?,
                    });
                }
                indices = Some(v);
            }
            _ => {
                return Err(format!("Invalid TriangleStripArray encoding ({}).", self.resource_name));
            }
        }
        let strip_count = r.u32()? as usize;
        let mut strip_lengths = Vec::new();
        for _ in 0..strip_count {
            strip_lengths.push(r.u32()? as i32);
        }
        if strip_lengths.is_empty() || strip_lengths.iter().any(|&l| l < 3) {
            return Err("TriangleStripArray: strip lengths must be at least 3".into());
        }
        let sum: i64 = strip_lengths.iter().map(|&l| l as i64).sum();
        let indices = match indices {
            None => {
                if first_index as i64 + sum > 65535 {
                    return Err("TriangleStripArray: indices exceed 65535".into());
                }
                (0..sum as i32).map(|i| first_index + i).collect()
            }
            Some(v) => {
                if (v.len() as i64) < sum {
                    return Err("TriangleStripArray: not enough indices".into());
                }
                let used = &v[..sum as usize];
                if used.iter().any(|&i| !(0..=65535).contains(&i)) {
                    return Err("TriangleStripArray: index out of range".into());
                }
                used.to_vec()
            }
        };
        let id = self.scene.add(Kind::TriangleStripArray(TriangleStripData { indices, strip_lengths }));
        self.finish(id, data)
    }

    fn read_vertex_array(&mut self, r: &mut Cursor) -> Result<Id> {
        let data = self.read_object3d_data(r)?;
        let component_size = r.byte()?;
        let component_count = r.byte()? as usize;
        let encoding = r.byte()?;
        let vertex_count = r.i16()? as usize;
        if encoding != 0 && encoding != 1 {
            return Err(format!("Invalid VertexArray encoding ({}).", self.resource_name));
        }
        if !(1..=65535).contains(&vertex_count) {
            return Err("VertexArray: invalid vertex count".into());
        }
        let array = if component_size == 1 {
            let mut v = Vec::new();
            let mut acc = vec![0i32; component_count];
            for _ in 0..vertex_count {
                for c in 0..component_count {
                    let b = r.byte()? as u8 as i8;
                    if encoding == 0 {
                        v.push(b);
                    } else {
                        acc[c] += b as i32;
                        v.push(acc[c] as i8);
                    }
                }
            }
            ArrayData::Bytes(v)
        } else {
            let mut v = Vec::new();
            let mut acc = vec![0i32; component_count];
            for _ in 0..vertex_count {
                for c in 0..component_count {
                    let s = r.i16()? as u16 as i16;
                    if encoding == 0 {
                        v.push(s);
                    } else {
                        acc[c] += s as i32;
                        v.push(acc[c] as i16);
                    }
                }
            }
            ArrayData::Shorts(v)
        };
        let id = self.scene.add(Kind::VertexArray(VertexArrayData {
            vertex_count,
            component_count,
            data: array,
        }));
        self.finish(id, data)
    }

    fn array_info(&self, id: Id) -> (usize, usize, i32) {
        match self.scene.kind(id) {
            Kind::VertexArray(a) => (a.vertex_count, a.component_count, a.component_size()),
            _ => unreachable!("checked by reference_as"),
        }
    }

    fn read_vertex_buffer(&mut self, r: &mut Cursor) -> Result<Id> {
        let data = self.read_object3d_data(r)?;
        let mut vb = VertexBufferData::default();
        vb.default_color = r.rgba()?;
        let is_array = |k: &Kind| matches!(k, Kind::VertexArray(_));

        let positions = self.reference_as_opt(r, "VertexArray", is_array)?;
        let mut bias = [0.0f32; 3];
        for b in bias.iter_mut() {
            *b = r.f32()?;
        }
        let position_scale = r.f32()?;
        let mut vertex_count = 0usize;
        if let Some(p) = positions {
            let (count, comps, _) = self.array_info(p);
            if comps != 3 {
                return Err("VertexBuffer.setPositions: needs 3 components".into());
            }
            vertex_count = count;
            vb.positions = Some(p);
            vb.pos_scale_bias = [position_scale, bias[0], bias[1], bias[2]];
        }

        if let Some(n) = self.reference_as_opt(r, "VertexArray", is_array)? {
            let (count, comps, _) = self.array_info(n);
            if comps != 3 || (vertex_count != 0 && count != vertex_count) {
                return Err("VertexBuffer.setNormals: invalid array".into());
            }
            vertex_count = count;
            vb.normals = Some(n);
        }

        if let Some(c) = self.reference_as_opt(r, "VertexArray", is_array)? {
            let (count, comps, size) = self.array_info(c);
            if size != 1 || (comps != 3 && comps != 4) || (vertex_count != 0 && count != vertex_count) {
                return Err("VertexBuffer.setColors: invalid array".into());
            }
            vertex_count = count;
            vb.colors = Some(c);
        }

        let tex_count = r.u32()? as usize;
        for unit in 0..tex_count {
            let t = self.reference_as(r, "VertexArray", is_array)?;
            for b in bias.iter_mut() {
                *b = r.f32()?;
            }
            let scale = r.f32()?;
            let (count, comps, _) = self.array_info(t);
            if (comps != 2 && comps != 3) || (vertex_count != 0 && count != vertex_count) {
                return Err("VertexBuffer.setTexCoords: invalid array".into());
            }
            if unit >= MAX_TEXTURE_UNITS {
                return Err("VertexBuffer.setTexCoords: index out of bounds".into());
            }
            vertex_count = count;
            vb.tex_coords[unit] = Some(t);
            let mut sb = [scale, 0.0, 0.0, 0.0];
            sb[1..=comps].copy_from_slice(&bias[..comps]);
            vb.uv_scale_bias[unit] = sb;
        }
        vb.vertex_count = vertex_count;
        let id = self.scene.add(Kind::VertexBuffer(vb));
        self.finish(id, data)
    }
}

fn valid_component_count(property: i32, count: usize) -> bool {
    match property {
        prop::ALPHA => count == 1,
        prop::AMBIENT_COLOR | prop::COLOR | prop::DIFFUSE_COLOR | prop::EMISSIVE_COLOR
        | prop::SPECULAR_COLOR => count == 3,
        prop::CROP => count == 2 || count == 4,
        prop::DENSITY | prop::FAR_DISTANCE | prop::FIELD_OF_VIEW | prop::INTENSITY
        | prop::NEAR_DISTANCE | prop::PICKABILITY | prop::SHININESS | prop::SPOT_ANGLE
        | prop::SPOT_EXPONENT | prop::VISIBILITY => count == 1,
        prop::MORPH_WEIGHTS => true,
        prop::ORIENTATION => count == 4,
        prop::SCALE => count == 1 || count == 3,
        prop::TRANSLATION => count == 3,
        _ => false,
    }
}

/// Picks the scene's root the way the viewer does: the last `World`, else the
/// first other node. Returns `(root, world)`.
pub fn pick_root(scene: &Scene, roots: &[Id]) -> (Option<Id>, Option<Id>) {
    let mut world = None;
    let mut root = None;
    for &id in roots {
        if matches!(scene.kind(id), Kind::World(_)) {
            world = Some(id);
            root = Some(id);
        } else if scene.is_node(id) && root.is_none() {
            root = Some(id);
        }
    }
    (root, world)
}
