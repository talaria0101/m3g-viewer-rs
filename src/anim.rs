//! Keyframe sequences and the animation pass (`Object3D.animate`).

use std::collections::HashSet;

use crate::math::{self, Quat};
use crate::scene::*;

pub struct KeyframeSequence {
    pub component_count: usize,
    keyframe_count: usize,
    interpolation: i32,
    values: Vec<Vec<f32>>,
    times: Vec<i32>,
    valid_first: usize,
    valid_last: usize,
    duration: i32,
    repeat_mode: i32,
    dirty: bool,
    in_tangent: Vec<Vec<f32>>,
    out_tangent: Vec<Vec<f32>>,
    squad_out: Vec<Quat>,
    squad_in: Vec<Quat>,
}

impl KeyframeSequence {
    pub fn new(
        keyframes: usize,
        components: usize,
        interpolation: i32,
    ) -> Result<KeyframeSequence> {
        let valid_interp = (KF_LINEAR..=KF_STEP).contains(&interpolation);
        if keyframes < 1 || components < 1 || !valid_interp {
            return Err("KeyframeSequence: invalid arguments".into());
        }
        if (interpolation == KF_SLERP || interpolation == KF_SQUAD) && components != 4 {
            return Err("KeyframeSequence: SLERP/SQUAD need 4 components".into());
        }
        let mut s = KeyframeSequence {
            component_count: components,
            keyframe_count: keyframes,
            interpolation,
            values: vec![vec![0.0; components]; keyframes],
            times: vec![0; keyframes],
            valid_first: 0,
            valid_last: keyframes - 1,
            duration: 0,
            repeat_mode: KF_CONSTANT,
            dirty: false,
            in_tangent: Vec::new(),
            out_tangent: Vec::new(),
            squad_out: Vec::new(),
            squad_in: Vec::new(),
        };
        if interpolation == KF_SPLINE {
            s.in_tangent = vec![vec![0.0; components]; keyframes];
            s.out_tangent = vec![vec![0.0; components]; keyframes];
        } else if interpolation == KF_SQUAD {
            s.squad_out = vec![Quat::new(0.0, 0.0, 0.0, 0.0); keyframes];
            s.squad_in = vec![Quat::new(0.0, 0.0, 0.0, 0.0); keyframes];
        }
        Ok(s)
    }

    pub fn set_keyframe(&mut self, index: usize, time: i32, value: &[f32]) -> Result<()> {
        if index >= self.keyframe_count {
            return Err("KeyframeSequence.setKeyframe: index out of bounds".into());
        }
        if time < 0 || value.len() < self.component_count {
            return Err("KeyframeSequence.setKeyframe: invalid arguments".into());
        }
        self.times[index] = time;
        if self.interpolation != KF_SLERP && self.interpolation != KF_SQUAD {
            let n = self.values[index].len();
            self.values[index].copy_from_slice(&value[..n]);
        } else {
            let mut q = Quat::new(value[0], value[1], value[2], value[3]);
            q.normalize();
            self.values[index] = vec![q.x, q.y, q.z, q.w];
        }
        self.dirty = false;
        Ok(())
    }

    pub fn set_valid_range(&mut self, first: i32, last: i32) -> Result<()> {
        let n = self.keyframe_count as i32;
        if first >= 0 && first < n && last >= 0 && last < n {
            self.valid_first = first as usize;
            self.valid_last = last as usize;
            self.dirty = false;
            Ok(())
        } else {
            Err("KeyframeSequence.setValidRange: out of bounds".into())
        }
    }

    pub fn set_duration(&mut self, duration: i32) {
        self.duration = duration;
    }

    pub fn set_repeat_mode(&mut self, mode: i32) -> Result<()> {
        if mode != KF_CONSTANT && mode != KF_LOOP {
            return Err("KeyframeSequence.setRepeatMode: invalid mode".into());
        }
        self.repeat_mode = mode;
        if self.dirty {
            self.update_tangents();
        }
        Ok(())
    }

    /// `getSampleFrame`: writes the interpolated value at `time` into `sample` and
    /// returns the time until the sample next changes.
    pub fn sample(&mut self, time: f32, sample: &mut [f32]) -> Result<i32> {
        if !self.dirty {
            self.validate()?;
        }

        let mut time = time;
        let duration = self.duration as f32;
        if self.repeat_mode == KF_LOOP {
            time = if time < 0.0 {
                time % duration + duration
            } else {
                time % duration
            };
            if time < self.times[self.valid_first] as f32 {
                time += duration;
            }
        } else {
            if time < self.times[self.valid_first] as f32 {
                let kf = &self.values[self.valid_first];
                sample[..kf.len()].copy_from_slice(kf);
                let delta = self.times[self.valid_first] as f32 - time;
                if delta <= 2.14748365E9f32 {
                    return Ok(delta as i32);
                }
                return Ok(i32::MAX);
            }
            if time >= self.times[self.valid_last] as f32 {
                let kf = &self.values[self.valid_last];
                sample[..kf.len()].copy_from_slice(kf);
                return Ok(i32::MAX);
            }
        }

        let mut next_index = self.valid_first;
        loop {
            let frame = next_index;
            if next_index == self.valid_last || self.times[self.after(frame)] as f32 > time {
                if time - self.times[frame] as f32 >= 1.0E-5 && self.interpolation != KF_STEP {
                    let delta = (time - self.times[frame] as f32) / (self.time_delta(frame) as f32);
                    let next_frame = self.after(frame);
                    match self.interpolation {
                        KF_LINEAR => self.linear(sample, delta, frame, next_frame),
                        KF_SLERP => self.slerp(sample, delta, frame, next_frame),
                        KF_SPLINE => self.spline(sample, delta, frame, next_frame),
                        KF_SQUAD => self.squad(sample, delta, frame, next_frame),
                        _ => return Err("Invalid type for interpolation!".into()),
                    }
                    return Ok(1);
                } else {
                    let kf = &self.values[frame];
                    sample[..self.component_count].copy_from_slice(&kf[..self.component_count]);
                    return Ok(if self.interpolation != KF_STEP {
                        1
                    } else {
                        ((self.time_delta(frame) as f32) - (time - self.times[frame] as f32)) as i32
                    });
                }
            }
            next_index = self.after(frame);
        }
    }

    fn validate(&mut self) -> Result<()> {
        if self.duration <= 0 {
            return Err("KeyframeSequence: duration must be positive".into());
        }
        let mut next_index = self.valid_first;
        loop {
            let frame = next_index;
            if next_index == self.valid_last {
                self.dirty = true;
                self.update_tangents();
                return Ok(());
            }
            let next = if frame >= self.times.len() - 1 {
                0
            } else {
                frame + 1
            };
            if self.times[next] < self.times[frame] || self.times[next] > self.duration {
                return Err(
                    "KeyframeSequence: keyframe times are not ascending within the duration".into(),
                );
            }
            next_index = next;
        }
    }

    fn linear(&self, out: &mut [f32], weight: f32, a: usize, b: usize) {
        let fa = &self.values[a];
        let fb = &self.values[b];
        for i in 0..out.len() {
            out[i] = fa[i] + weight * (fb[i] - fa[i]);
        }
    }

    fn spline(&self, out: &mut [f32], weight: f32, a: usize, b: usize) {
        let fa = &self.values[a];
        let fb = &self.values[b];
        for i in 0..out.len() {
            let ta = self.out_tangent[a][i];
            let tb = self.in_tangent[b][i];
            out[i] = spline(weight, fa[i], fb[i], ta, tb);
        }
    }

    fn slerp(&self, out: &mut [f32], weight: f32, a: usize, b: usize) {
        let qa = Quat::from_slice(&self.values[a]);
        let qb = Quat::from_slice(&self.values[b]);
        let mut r = Quat::new(0.0, 0.0, 0.0, 0.0);
        r.slerp(weight, &qa, &qb);
        out[..4].copy_from_slice(&[r.x, r.y, r.z, r.w]);
    }

    fn squad(&self, out: &mut [f32], weight: f32, a: usize, b: usize) {
        let qa = Quat::from_slice(&self.values[a]);
        let qb = Quat::from_slice(&self.values[b]);
        let mut r = Quat::new(0.0, 0.0, 0.0, 0.0);
        r.squad(weight, &qa, &self.squad_out[a], &self.squad_in[b], &qb);
        out[..4].copy_from_slice(&[r.x, r.y, r.z, r.w]);
    }

    fn precalculate_tangents(&mut self) {
        let mut frame = self.valid_first;
        loop {
            let prev = self.values[self.before(frame)].clone();
            let next = self.values[self.after(frame)].clone();
            let in_scale = self.incoming_scale(frame);
            let out_scale = self.outgoing_scale(frame);
            for i in 0..self.component_count {
                self.in_tangent[frame][i] = 0.5f32 * (next[i] - prev[i]) * in_scale;
                self.out_tangent[frame][i] = 0.5f32 * (next[i] - prev[i]) * out_scale;
            }
            frame = self.after(frame);
            if frame == self.valid_first {
                break;
            }
        }
    }

    fn precalculate_ab(&mut self) {
        let mut tmp = Quat::new(0.0, 0.0, 0.0, 0.0);
        let mut tangent = Quat::new(0.0, 0.0, 0.0, 0.0);
        let mut frame = self.valid_first;
        loop {
            let previous = Quat::from_slice(&self.values[self.before(frame)]);
            let current = Quat::from_slice(&self.values[frame]);
            let next = Quat::from_slice(&self.values[self.after(frame)]);
            // `afterNext` is read by the original but never used.
            tangent.log_diff(&current, &next);
            tmp.log_diff(&previous, &current);
            tangent.add(&tmp);
            tangent.mul_scalar(0.5);
            let mut scaled = tangent;
            scaled.mul_scalar(self.outgoing_scale(frame));
            tmp.log_diff(&current, &next);
            scaled.sub(&tmp);
            scaled.mul_scalar(0.5);
            tmp.exp(&scaled);
            self.squad_out[frame] = current;
            self.squad_out[frame].mul(&tmp);
            let mut scaled = tangent;
            scaled.mul_scalar(self.incoming_scale(frame));
            tmp.log_diff(&previous, &current);
            tmp.sub(&scaled);
            tmp.mul_scalar(0.5);
            let t = tmp;
            tmp.exp(&t);
            self.squad_in[frame] = current;
            self.squad_in[frame].mul(&tmp);
            frame = self.after(frame);
            if frame == self.valid_first {
                break;
            }
        }
    }

    fn incoming_scale(&self, frame: usize) -> f32 {
        if self.repeat_mode != KF_CONSTANT
            || (frame != self.valid_first && frame != self.valid_last)
        {
            let prev = self.before(frame);
            2.0f32 * (self.time_delta(prev) as f32)
                / (self.time_delta(frame).wrapping_add(self.time_delta(prev)) as f32)
        } else {
            0.0
        }
    }

    fn outgoing_scale(&self, frame: usize) -> f32 {
        if self.repeat_mode != KF_CONSTANT
            || (frame != self.valid_first && frame != self.valid_last)
        {
            let prev = self.before(frame);
            2.0f32 * (self.time_delta(frame) as f32)
                / (self.time_delta(prev).wrapping_add(self.time_delta(frame)) as f32)
        } else {
            0.0
        }
    }

    fn update_tangents(&mut self) {
        if self.interpolation == KF_SPLINE {
            self.precalculate_tangents();
        } else if self.interpolation == KF_SQUAD {
            self.precalculate_ab();
        }
    }

    fn after(&self, frame: usize) -> usize {
        if frame == self.valid_last {
            self.valid_first
        } else if frame == self.times.len() - 1 {
            0
        } else {
            frame + 1
        }
    }

    fn before(&self, frame: usize) -> usize {
        if frame == self.valid_first {
            self.valid_last
        } else if frame == 0 {
            self.times.len() - 1
        } else {
            frame - 1
        }
    }

    fn time_delta(&self, frame: usize) -> i32 {
        if frame == self.valid_last {
            self.duration
                .wrapping_sub(self.times[self.valid_last])
                .wrapping_add(self.times[self.valid_first])
        } else {
            self.times[self.after(frame)].wrapping_sub(self.times[frame])
        }
    }
}

fn spline(weight: f32, a: f32, b: f32, tangent_a: f32, tangent_b: f32) -> f32 {
    let w2 = weight * weight;
    let w3 = w2 * weight;
    (2.0f32 * w3 - 3.0f32 * w2 + 1.0f32) * a
        + (-2.0f32 * w3 + 3.0f32 * w2) * b
        + (w3 - 2.0f32 * w2 + weight) * tangent_a
        + (w3 - w2) * tangent_b
}

impl Scene {
    /// `World.postFrame`: applies every animation track reachable from `root`
    /// at `time`. Each object is visited once; the pass is idempotent for a
    /// given time, so revisiting (as the original does) changes nothing.
    pub fn animate(&mut self, root: Id, time: i32) -> Result<()> {
        let mut seen = HashSet::new();
        let mut stack = vec![root];
        while let Some(id) = stack.pop() {
            if !seen.insert(id) {
                continue;
            }
            self.animation(id, time)?;
            stack.extend(self.references(id));
        }
        Ok(())
    }

    /// `Object3D.animation`: runs this object's own tracks.
    fn animation(&mut self, id: Id, time: i32) -> Result<()> {
        let tracks = self.objs[id].tracks.clone();
        let mut index = 0;
        while index < tracks.len() {
            let property = self.track_data(tracks[index]).property;
            let components = self.sequence_components(tracks[index]);
            let mut value = vec![0.0f32; components];
            let mut weight_sum = 0.0f32;

            loop {
                let weight = self.track_contribution(tracks[index], time, &mut value)?;
                weight_sum += weight;
                index += 1;
                if index == tracks.len() || self.track_data(tracks[index]).property != property {
                    break;
                }
            }

            if weight_sum > 0.0 {
                self.update_property(id, property, &value)?;
            }
        }
        Ok(())
    }

    fn track_data(&self, track: Id) -> TrackData {
        match &self.objs[track].kind {
            Kind::AnimationTrack(t) => t.clone(),
            _ => unreachable!("object tracks only hold AnimationTrack ids"),
        }
    }

    fn sequence_components(&self, track: Id) -> usize {
        match &self.objs[self.track_data(track).sequence].kind {
            Kind::KeyframeSequence(k) => k.component_count,
            _ => unreachable!("tracks only reference KeyframeSequence"),
        }
    }

    /// `AnimationTrack.getContribution`: adds this track's weighted sample to
    /// `value` and returns the weight it contributed.
    fn track_contribution(&mut self, track: Id, time: i32, value: &mut [f32]) -> Result<f32> {
        let t = self.track_data(track);
        let controller = match t.controller {
            Some(c) => match &self.objs[c].kind {
                Kind::AnimationController(c) => Some(c.clone()),
                _ => return Err("track controller is not an AnimationController".into()),
            },
            None => None,
        };
        match controller {
            Some(c) if c.is_active(time) => {
                let mut components = vec![0.0f32; value.len()];
                let position = c.get_position(time);
                match &mut self.objs[t.sequence].kind {
                    Kind::KeyframeSequence(k) => {
                        k.sample(position, &mut components)?;
                    }
                    _ => unreachable!("tracks only reference KeyframeSequence"),
                }
                for i in 0..components.len() {
                    value[i] += components[i] * c.weight;
                }
                Ok(c.weight)
            }
            _ => Ok(0.0),
        }
    }

    /// `updateProperty` dispatch, following the original's override chain.
    fn update_property(&mut self, id: Id, property: i32, v: &[f32]) -> Result<()> {
        let bad = |kind: &str| -> Result<()> {
            Err(format!(
                "Invalid animation target property {property} for {kind}"
            ))
        };
        let kind_name = self.objs[id].kind.name();
        match &mut self.objs[id].kind {
            Kind::MorphingMesh(m) if property == prop::MORPH_WEIGHTS => {
                m.base_weight = 1.0;
                for i in 0..m.weights.len() {
                    if i < v.len() {
                        m.weights[i] = v[i];
                        m.base_weight -= v[i];
                    } else {
                        m.weights[i] = 0.0;
                    }
                }
                Ok(())
            }
            Kind::Camera(c) if c.projection_type != CAMERA_GENERIC => {
                let perspective = c.projection_type == CAMERA_PERSPECTIVE;
                match property {
                    prop::FAR_DISTANCE => {
                        c.projection[3] = if perspective {
                            math::limit_positive(v[0])
                        } else {
                            v[0]
                        };
                        Ok(())
                    }
                    prop::FIELD_OF_VIEW => {
                        c.projection[0] = if perspective {
                            math::limit(v[0], 0.0, 180.0)
                        } else {
                            math::limit_positive(v[0])
                        };
                        Ok(())
                    }
                    prop::NEAR_DISTANCE => {
                        c.projection[2] = if perspective {
                            math::limit_positive(v[0])
                        } else {
                            v[0]
                        };
                        Ok(())
                    }
                    _ => Self::update_node_property(&mut c.node, property, v)
                        .or_else(|_| bad(kind_name)),
                }
            }
            Kind::Light(l) => match property {
                prop::COLOR => {
                    l.color = math::int_color(v);
                    Ok(())
                }
                prop::INTENSITY => {
                    l.intensity = v[0];
                    Ok(())
                }
                prop::SPOT_ANGLE => {
                    l.spot_angle = math::limit(v[0], 0.0, 90.0);
                    Ok(())
                }
                prop::SPOT_EXPONENT => {
                    l.spot_exponent = math::limit(v[0], 0.0, 128.0);
                    Ok(())
                }
                _ => {
                    Self::update_node_property(&mut l.node, property, v).or_else(|_| bad(kind_name))
                }
            },
            Kind::Material(m) => {
                match property {
                    prop::ALPHA => {
                        m.diffuse = (m.diffuse & 0x00FF_FFFF)
                            | (math::int_color(v) & (0xFF00_0000u32 as i32));
                    }
                    prop::AMBIENT_COLOR => m.ambient = math::int_color(v),
                    prop::DIFFUSE_COLOR => {
                        m.diffuse = (m.diffuse & (0xFF00_0000u32 as i32))
                            | (math::int_color(v) & 0x00FF_FFFF);
                    }
                    prop::EMISSIVE_COLOR => m.emissive = math::int_color(v),
                    prop::SHININESS => m.shininess = math::limit(v[0], 0.0, 128.0),
                    prop::SPECULAR_COLOR => m.specular = math::int_color(v),
                    _ => return bad(kind_name),
                }
                Ok(())
            }
            Kind::Fog(f) => {
                match property {
                    prop::COLOR => f.color = math::int_color(v),
                    prop::DENSITY => f.density = math::limit_positive(v[0]),
                    prop::FAR_DISTANCE => f.far = v[0],
                    prop::NEAR_DISTANCE => f.near = v[0],
                    _ => return bad(kind_name),
                }
                Ok(())
            }
            Kind::Texture2D(t) => {
                if property == prop::COLOR {
                    t.blend_color = math::int_color(v);
                    Ok(())
                } else if t.tr.update_property(property, v) {
                    Ok(())
                } else {
                    bad(kind_name)
                }
            }
            Kind::VertexBuffer(vb) => {
                match property {
                    prop::ALPHA => {
                        vb.default_color &= 0x00FF_FFFF;
                        vb.default_color |= math::int_color(v) & (0xFF00_0000u32 as i32);
                    }
                    prop::COLOR => {
                        vb.default_color &= 0xFF00_0000u32 as i32;
                        vb.default_color |= math::int_color(v) & 0x00FF_FFFF;
                    }
                    _ => return bad(kind_name),
                }
                Ok(())
            }
            Kind::Background(b) => {
                match property {
                    prop::ALPHA => {
                        b.color = (b.color & 0x00FF_FFFF)
                            | (math::int_color(v) & (0xFF00_0000u32 as i32));
                    }
                    prop::COLOR => {
                        b.color = (b.color & (0xFF00_0000u32 as i32))
                            | (math::int_color(v) & 0x00FF_FFFF);
                    }
                    prop::CROP => {
                        b.crop[0] = math::round(v[0]);
                        b.crop[1] = math::round(v[1]);
                        if v.len() > 2 {
                            b.crop[2] = math::round(v[2]).max(0);
                            b.crop[3] = math::round(v[3]).max(0);
                        }
                    }
                    _ => return bad(kind_name),
                }
                Ok(())
            }
            Kind::Sprite3D(s) if property == prop::CROP => {
                s.crop[0] = math::round(v[0]);
                s.crop[1] = math::round(v[1]);
                if v.len() > 2 {
                    s.crop[2] = math::limit_i(math::round(v[2]), -1024, 1024);
                    s.crop[3] = math::limit_i(math::round(v[3]), -1024, 1024);
                }
                Ok(())
            }
            _ => match self.node_base_mut(id) {
                Some(n) => Self::update_node_property(n, property, v).or_else(|_| bad(kind_name)),
                None => bad(kind_name),
            },
        }
    }

    fn update_node_property(
        n: &mut NodeBase,
        property: i32,
        v: &[f32],
    ) -> std::result::Result<(), ()> {
        match property {
            prop::ALPHA => {
                n.alpha_factor = math::limit(v[0], 0.0, 1.0);
                Ok(())
            }
            prop::PICKABILITY => {
                n.picking = v[0] >= 0.5;
                Ok(())
            }
            prop::VISIBILITY => {
                n.rendering = v[0] >= 0.5;
                Ok(())
            }
            _ => {
                if n.tr.update_property(property, v) {
                    Ok(())
                } else {
                    Err(())
                }
            }
        }
    }
}
