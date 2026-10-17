//! Flattens a scene graph into a sorted list of things to draw (`RenderPipe`).

use crate::math::Mat4;
use crate::scene::*;

pub struct RenderObject {
    pub node: Id,
    pub submesh: usize,
    pub trans: Mat4,
    pub sort_key: i32,
    pub alpha_factor: f32,
}

pub struct RenderPipe {
    parent_node: Option<Id>,
    pub list: Vec<RenderObject>,
    pub render_invisible_nodes: bool,
}

impl RenderPipe {
    pub fn new() -> RenderPipe {
        RenderPipe {
            parent_node: None,
            list: Vec::new(),
            render_invisible_nodes: false,
        }
    }

    pub fn clear(&mut self) {
        self.list.clear();
        self.parent_node = None;
    }

    pub fn is_visible(&self, scene: &Scene, node: Id) -> bool {
        let mut tmp = Some(node);
        while let Some(n) = tmp {
            let base = scene.node_base(n).expect("visibility is asked of nodes");
            if !base.rendering && !self.render_invisible_nodes {
                return false;
            }
            if self.parent_node.is_none() || Some(n) == self.parent_node {
                break;
            }
            tmp = base.parent;
        }
        true
    }

    /// `getEffectiveAlphaFactor`. The stop test compares the *original* node with
    /// the render root, not the node being visited, so it only ever stops early
    /// when the node itself is the root.
    pub fn effective_alpha_factor(&self, scene: &Scene, node: Id) -> f32 {
        let mut alpha = 1.0f32;
        let mut tmp = node;
        loop {
            let base = scene.node_base(tmp).expect("alpha is asked of nodes");
            alpha *= base.alpha_factor;
            match base.parent {
                None => return alpha,
                Some(_) if Some(node) == self.parent_node => return alpha,
                Some(p) => tmp = p,
            }
        }
    }

    pub fn push_render_node(&mut self, scene: &Scene, node: Id, trans: Mat4) {
        self.parent_node = Some(node);
        self.push_impl(scene, node, trans);
    }

    fn sort_key(scene: &Scene, appearance: Id) -> i32 {
        let ap = match scene.kind(appearance) {
            Kind::Appearance(a) => a,
            _ => unreachable!("appearances are Appearance objects"),
        };
        let mut key = ap.layer * 2;
        if let Some(cm) = ap.compositing_mode {
            if let Kind::CompositingMode(c) = scene.kind(cm) {
                if c.blending != BLEND_REPLACE {
                    key += 1;
                }
            }
        }
        key
    }

    fn push_impl(&mut self, scene: &Scene, node: Id, trans: Mat4) {
        if !self.is_visible(scene, node) {
            return;
        }
        match scene.kind(node) {
            Kind::Sprite3D(s) => {
                if let Some(ap) = s.appearance {
                    if s.crop[2] != 0 && s.crop[3] != 0 {
                        self.list.push(RenderObject {
                            node,
                            submesh: 0,
                            trans,
                            sort_key: Self::sort_key(scene, ap),
                            alpha_factor: self.effective_alpha_factor(scene, node),
                        });
                    }
                }
            }
            Kind::Mesh(_) | Kind::MorphingMesh(_) | Kind::SkinnedMesh(_) => {
                let mesh = scene.mesh(node).expect("mesh kinds have mesh data");
                for (i, ap) in mesh.appearances.iter().enumerate() {
                    if let Some(ap) = ap {
                        self.list.push(RenderObject {
                            node,
                            submesh: i,
                            trans,
                            sort_key: Self::sort_key(scene, *ap),
                            alpha_factor: self.effective_alpha_factor(scene, node),
                        });
                    }
                }
                if let Kind::SkinnedMesh(s) = scene.kind(node) {
                    let skeleton_mat = scene.composite_transform(s.skeleton);
                    let mut tmp = trans;
                    tmp.post_multiply(&skeleton_mat, false);
                    self.push_impl(scene, s.skeleton, tmp);
                }
            }
            Kind::Group(_) | Kind::World(_) => {
                let group = scene.group(node).expect("group kinds have group data");
                let group_trans = trans;
                for &child in &group.children {
                    let mut child_trans = scene.composite_transform(child);
                    child_trans.post_multiply(&group_trans, true);
                    self.push_impl(scene, child, child_trans);
                }
            }
            _ => {}
        }
    }

    /// Stable sort by key, as `List.sort` is in the original.
    pub fn sort(&mut self) {
        self.list.sort_by_key(|o| o.sort_key);
    }
}
