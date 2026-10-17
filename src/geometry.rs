//! Resolves a mesh's vertex data for drawing: plain, morphed or skinned.
//!
//! The result keeps the original's integer quantisation: morphing and skinning
//! write back into byte/short arrays, so the values here are integers stored as
//! floats and must not be "improved" by keeping extra precision.

use crate::math::{self, Mat4};
use crate::scene::*;

pub struct TexSet {
    pub components: usize,
    pub data: Vec<f32>,
    /// `[scale, bias s, bias t, bias r]` as the renderer's `scaleBias` array holds it.
    pub scale_bias: [f32; 4],
}

pub struct ResolvedVb {
    pub vertex_count: usize,
    pub positions: Vec<f32>,
    /// `[scale, bias x, bias y, bias z]`.
    pub pos_scale_bias: [f32; 4],
    pub normals: Option<Vec<f32>>,
    pub colors: Option<(Vec<u8>, usize)>,
    pub default_color: i32,
    pub tex: [Option<TexSet>; MAX_TEXTURE_UNITS],
}

fn array<'a>(scene: &'a Scene, id: Id) -> &'a VertexArrayData {
    match scene.kind(id) {
        Kind::VertexArray(a) => a,
        _ => unreachable!("vertex buffers only reference VertexArray"),
    }
}

fn vertex_buffer<'a>(scene: &'a Scene, id: Id) -> &'a VertexBufferData {
    match scene.kind(id) {
        Kind::VertexBuffer(v) => v,
        _ => unreachable!("meshes only reference VertexBuffer"),
    }
}

fn values(a: &VertexArrayData) -> Vec<f32> {
    match &a.data {
        ArrayData::Bytes(b) => b.iter().map(|&v| v as f32).collect(),
        ArrayData::Shorts(s) => s.iter().map(|&v| v as f32).collect(),
    }
}

fn color_bytes(a: &VertexArrayData) -> Vec<u8> {
    match &a.data {
        ArrayData::Bytes(b) => b.iter().map(|&v| v as u8).collect(),
        // setColors rejects anything but byte components.
        ArrayData::Shorts(_) => unreachable!("colour arrays are byte arrays"),
    }
}

fn tex_set(vb: &VertexBufferData, unit: usize, a: &VertexArrayData) -> TexSet {
    TexSet {
        components: a.component_count,
        data: values(a),
        scale_bias: vb.uv_scale_bias[unit],
    }
}

fn plain(scene: &Scene, vb_id: Id) -> Result<ResolvedVb> {
    let vb = vertex_buffer(scene, vb_id);
    let positions = vb.positions.ok_or("mesh has no vertex positions")?;
    let mut tex: [Option<TexSet>; MAX_TEXTURE_UNITS] = Default::default();
    for (unit, slot) in tex.iter_mut().enumerate() {
        if let Some(t) = vb.tex_coords[unit] {
            *slot = Some(tex_set(vb, unit, array(scene, t)));
        }
    }
    Ok(ResolvedVb {
        vertex_count: vb.vertex_count,
        positions: values(array(scene, positions)),
        pos_scale_bias: vb.pos_scale_bias,
        normals: vb.normals.map(|n| values(array(scene, n))),
        colors: vb.colors.map(|c| {
            let a = array(scene, c);
            (color_bytes(a), a.component_count)
        }),
        default_color: vb.default_color,
        tex,
    })
}

pub fn resolve(scene: &Scene, mesh: Id) -> Result<ResolvedVb> {
    match scene.kind(mesh) {
        Kind::MorphingMesh(m) => morph(scene, m),
        Kind::SkinnedMesh(s) => skin(scene, mesh, s),
        _ => plain(scene, scene.mesh(mesh).ok_or("not a mesh")?.vertices),
    }
}

// ------------------------------------------------------------------ morphing

fn same_shape(a: &VertexArrayData, b: &VertexArrayData) -> bool {
    a.component_size() == b.component_size()
        && a.component_count == b.component_count
        && a.vertex_count == b.vertex_count
}

/// `VertexArray.morph`: integer result, rounded and truncated to the array's width.
fn morph_array(
    base_copy: &VertexArrayData,
    targets: &[&VertexArrayData],
    base: &VertexArrayData,
    weights: &[f32],
    base_weight: f32,
) -> Result<VertexArrayData> {
    for t in targets {
        if !same_shape(base_copy, t) {
            return Err("morph target does not match the base array".into());
        }
    }
    let mut out = base_copy.clone();
    match (&mut out.data, &base.data) {
        (ArrayData::Bytes(o), ArrayData::Bytes(b)) => {
            for i in 0..o.len() {
                let mut val = 0.0f32;
                for (t, target) in targets.iter().enumerate() {
                    let tv = match &target.data {
                        ArrayData::Bytes(v) => v[i] as f32,
                        _ => unreachable!("shape checked"),
                    };
                    val += tv * weights[t];
                }
                val += (b[i] as f32) * base_weight;
                o[i] = math::round(val) as i8;
            }
        }
        (ArrayData::Shorts(o), ArrayData::Shorts(b)) => {
            for i in 0..o.len() {
                let mut val = 0.0f32;
                for (t, target) in targets.iter().enumerate() {
                    let tv = match &target.data {
                        ArrayData::Shorts(v) => v[i] as f32,
                        _ => unreachable!("shape checked"),
                    };
                    val += tv * weights[t];
                }
                val += (b[i] as f32) * base_weight;
                o[i] = math::round(val) as i16;
            }
        }
        _ => return Err("morph base does not match the array".into()),
    }
    Ok(out)
}

/// `VertexArray.morphColors`: unsigned bytes, clamped to 0..255.
fn morph_colors(
    base_copy: &VertexArrayData,
    targets: &[&VertexArrayData],
    base: &VertexArrayData,
    weights: &[f32],
    base_weight: f32,
) -> Result<VertexArrayData> {
    for t in targets {
        if !same_shape(base_copy, t) {
            return Err("morph target does not match the base colours".into());
        }
    }
    let mut out = base_copy.clone();
    if let (ArrayData::Bytes(o), ArrayData::Bytes(b)) = (&mut out.data, &base.data) {
        for i in 0..o.len() {
            let mut val = 0.0f32;
            for (t, target) in targets.iter().enumerate() {
                let tv = match &target.data {
                    ArrayData::Bytes(v) => (v[i] as u8) as f32,
                    _ => unreachable!("shape checked"),
                };
                val += tv * weights[t];
            }
            val += ((b[i] as u8) as f32) * base_weight;
            o[i] = math::limit_i(math::round(val), 0, 255) as u8 as i8;
        }
    }
    Ok(out)
}

/// One attribute of the morph: which of the `processMorph` blocks to run.
fn morph_attribute(
    scene: &Scene,
    base: Option<Id>,
    targets: &[Option<Id>],
    weights: &[f32],
    base_weight: f32,
    colors: bool,
) -> Result<Option<VertexArrayData>> {
    let Some(base_id) = base else {
        if targets.iter().any(|t| t.is_some()) {
            return Err("morph target has an array the base lacks".into());
        }
        return Ok(None);
    };
    let base_array = array(scene, base_id);
    let null_targets = targets.iter().filter(|t| t.is_none()).count();
    if null_targets != targets.len() && null_targets != 0 {
        return Err("morph targets disagree about an array".into());
    }
    if null_targets == 0 {
        let refs: Vec<&VertexArrayData> =
            targets.iter().map(|t| array(scene, t.unwrap())).collect();
        let out = if colors {
            morph_colors(base_array, &refs, base_array, weights, base_weight)?
        } else {
            morph_array(base_array, &refs, base_array, weights, base_weight)?
        };
        Ok(Some(out))
    } else {
        Ok(Some(base_array.clone()))
    }
}

fn morph(scene: &Scene, m: &MorphingData) -> Result<ResolvedVb> {
    let vb = vertex_buffer(scene, m.mesh.vertices);
    let positions_id = vb.positions.ok_or("mesh has no vertex positions")?;
    let target_vbs: Vec<&VertexBufferData> =
        m.targets.iter().map(|&t| vertex_buffer(scene, t)).collect();
    let base_weight = m.base_weight;
    let weights = &m.weights;

    let positions = morph_attribute(
        scene,
        Some(positions_id),
        &target_vbs.iter().map(|t| t.positions).collect::<Vec<_>>(),
        weights,
        base_weight,
        false,
    )?
    .expect("positions are present");

    let normals = morph_attribute(
        scene,
        vb.normals,
        &target_vbs.iter().map(|t| t.normals).collect::<Vec<_>>(),
        weights,
        base_weight,
        false,
    )?;

    let colors = morph_attribute(
        scene,
        vb.colors,
        &target_vbs.iter().map(|t| t.colors).collect::<Vec<_>>(),
        weights,
        base_weight,
        true,
    )?;

    let mut default_color = vb.default_color;
    if vb.colors.is_none() {
        let dc = vb.default_color;
        let mut a = base_weight * (((dc >> 24) & 255) as f32);
        let mut r = base_weight * (((dc >> 16) & 255) as f32);
        let mut g = base_weight * (((dc >> 8) & 255) as f32);
        let mut b = base_weight * ((dc & 255) as f32);
        for (i, t) in target_vbs.iter().enumerate() {
            let col = t.default_color;
            a += weights[i] * (((col >> 24) & 255) as f32);
            r += weights[i] * (((col >> 16) & 255) as f32);
            g += weights[i] * (((col >> 8) & 255) as f32);
            b += weights[i] * ((col & 255) as f32);
        }
        default_color = (math::limit_i(math::round(a), 0, 255) << 24)
            | (math::limit_i(math::round(r), 0, 255) << 16)
            | (math::limit_i(math::round(g), 0, 255) << 8)
            | math::limit_i(math::round(b), 0, 255);
    }

    let mut tex: [Option<TexSet>; MAX_TEXTURE_UNITS] = Default::default();
    for unit in 0..MAX_TEXTURE_UNITS {
        let t = morph_attribute(
            scene,
            vb.tex_coords[unit],
            &target_vbs
                .iter()
                .map(|t| t.tex_coords[unit])
                .collect::<Vec<_>>(),
            weights,
            base_weight,
            false,
        )?;
        if let Some(a) = t {
            tex[unit] = Some(tex_set(vb, unit, &a));
        }
    }

    Ok(ResolvedVb {
        vertex_count: vb.vertex_count,
        positions: values(&positions),
        pos_scale_bias: vb.pos_scale_bias,
        normals: normals.as_ref().map(values),
        colors: colors.as_ref().map(|c| (color_bytes(c), c.component_count)),
        default_color,
        tex,
    })
}

// ------------------------------------------------------------------- skinning

fn skin(scene: &Scene, mesh_id: Id, s: &SkinnedData) -> Result<ResolvedVb> {
    let vb = vertex_buffer(scene, s.mesh.vertices);
    let pos_id = vb.positions.ok_or("mesh has no vertex positions")?;
    let mesh_poses = array(scene, pos_id);
    let mesh_norms = vb.normals.map(|n| array(scene, n));
    let vertex_count = vb.vertex_count;
    let sb = vb.pos_scale_bias;

    let mut pos_trans: Vec<Mat4> = Vec::new();
    let mut norm_trans: Vec<Mat4> = Vec::new();
    for b in &s.bones {
        let mut pt = scene
            .get_transform_to(b.bone, mesh_id)
            .ok_or("skinning: bone is not in the mesh's tree")?;
        pt.post_multiply(&b.to_bone, false);
        let mut nt = pt;
        if mesh_norms.is_some() {
            nt.invert();
            nt.transpose();
        }
        pos_trans.push(pt);
        norm_trans.push(nt);
    }

    let poses = values(mesh_poses);
    let norms = mesh_norms.map(values);
    let norm_is_short = mesh_norms.map(|n| n.component_size() == 2).unwrap_or(false);
    let pos_is_short = mesh_poses.component_size() == 2;

    let mut tmp_positions = vec![0.0f32; vertex_count * 3];
    let mut tmp_normals = vec![0.0f32; vertex_count * 3];
    let mut norms_out = vec![
        0i16;
        if mesh_norms.is_some() {
            vertex_count * 3
        } else {
            0
        }
    ];
    let mut max_pos = [-f32::MAX; 3];
    let mut min_pos = [f32::MAX; 3];

    let per = MAX_TRANSFORMS_PER_VERTEX;
    for i in 0..vertex_count {
        let mut weight_sum = 0.0f32;
        for slot in 0..per {
            let bone_trans_id = s.vtx_bones[i * per + slot];
            if bone_trans_id == 0 {
                break;
            }
            let bone_weight = s.vtx_weights[i * per + slot];
            weight_sum += bone_weight as f32;

            let mut tmp = [0.0f32; 4];
            for axis in 0..3 {
                tmp[axis] = poses[i * 3 + axis];
            }
            tmp[0] = tmp[0] * sb[0] + sb[1];
            tmp[1] = tmp[1] * sb[0] + sb[2];
            tmp[2] = tmp[2] * sb[0] + sb[3];
            tmp[3] = 1.0;
            pos_trans[(bone_trans_id - 1) as usize].transform4(&mut tmp);
            tmp_positions[i * 3] += tmp[0] * (bone_weight as f32);
            tmp_positions[i * 3 + 1] += tmp[1] * (bone_weight as f32);
            tmp_positions[i * 3 + 2] += tmp[2] * (bone_weight as f32);

            if let Some(n) = &norms {
                for axis in 0..3 {
                    let raw = n[i * 3 + axis] as i32;
                    tmp[axis] = if norm_is_short {
                        ((raw + 32768) as f32) / 65535f32 - 0.5f32
                    } else {
                        ((raw + 128) as f32) / 255f32 - 0.5f32
                    };
                }
                tmp[3] = 0.0;
                norm_trans[(bone_trans_id - 1) as usize].transform4(&mut tmp);
                tmp_normals[i * 3] += tmp[0] * (bone_weight as f32);
                tmp_normals[i * 3 + 1] += tmp[1] * (bone_weight as f32);
                tmp_normals[i * 3 + 2] += tmp[2] * (bone_weight as f32);
            }
        }

        for axis in 0..3 {
            let pos_val = if weight_sum != 0.0 {
                tmp_positions[i * 3 + axis] / weight_sum
            } else {
                let raw = poses[i * 3 + axis];
                raw * sb[0] + sb[axis + 1]
            };
            tmp_positions[i * 3 + axis] = pos_val;
            if pos_val > max_pos[axis] {
                max_pos[axis] = pos_val;
            }
            if pos_val < min_pos[axis] {
                min_pos[axis] = pos_val;
            }

            if let Some(n) = &norms {
                let norm_val = if weight_sum != 0.0 {
                    tmp_normals[i * 3 + axis] / weight_sum
                } else {
                    let raw = n[i * 3 + axis] as i32;
                    if norm_is_short {
                        ((raw + 32768) as f32) / 65535f32 - 0.5f32
                    } else {
                        ((raw + 128) as f32) / 255f32 - 0.5f32
                    }
                };
                norms_out[i * 3 + axis] =
                    math::round((norm_val + 0.5f32) * 65535.0 - 32768.0) as i16;
            }
        }
    }

    let _ = pos_is_short;
    let mut bias = [0.0f32; 3];
    let mut max_axis_size = 0.0f32;
    for axis in 0..3 {
        bias[axis] = (min_pos[axis] + max_pos[axis]) / 2.0;
        let tmp_scale = (max_pos[axis] - min_pos[axis]) / 2.0;
        if max_axis_size < tmp_scale {
            max_axis_size = tmp_scale;
        }
    }
    let scale = if max_axis_size != 0.0 {
        max_axis_size / 32767.0
    } else {
        1.0
    };

    let mut positions = vec![0.0f32; vertex_count * 3];
    for i in 0..vertex_count {
        for axis in 0..3 {
            let v = math::round((tmp_positions[i * 3 + axis] - bias[axis]) / scale) as i16;
            positions[i * 3 + axis] = v as f32;
        }
    }

    let mut tex: [Option<TexSet>; MAX_TEXTURE_UNITS] = Default::default();
    for (unit, slot) in tex.iter_mut().enumerate() {
        if let Some(t) = vb.tex_coords[unit] {
            *slot = Some(tex_set(vb, unit, array(scene, t)));
        }
    }

    Ok(ResolvedVb {
        vertex_count,
        positions,
        pos_scale_bias: [scale, bias[0], bias[1], bias[2]],
        normals: mesh_norms.map(|_| norms_out.iter().map(|&v| v as f32).collect()),
        colors: vb.colors.map(|c| {
            let a = array(scene, c);
            (color_bytes(a), a.component_count)
        }),
        default_color: vb.default_color,
        tex,
    })
}
