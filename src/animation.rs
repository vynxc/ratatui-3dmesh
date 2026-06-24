//! Animation data and sampling helpers.
//!
//! glTF/GLB files can contain embedded node transform animation clips. OBJ and
//! STL are static formats, so loaders for those formats leave
//! [`crate::Mesh::animations`] empty.

use crate::model::{Bounds, Face, Mesh, Vec3};

/// Quaternion rotation stored as glTF-compatible `x, y, z, w` components.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Quaternion {
    /// X vector component.
    pub x: f32,
    /// Y vector component.
    pub y: f32,
    /// Z vector component.
    pub z: f32,
    /// Scalar component.
    pub w: f32,
}

impl Quaternion {
    /// Identity rotation.
    pub const IDENTITY: Self = Self {
        x: 0.0,
        y: 0.0,
        z: 0.0,
        w: 1.0,
    };

    /// Create a quaternion.
    #[must_use]
    pub const fn new(x: f32, y: f32, z: f32, w: f32) -> Self {
        Self { x, y, z, w }
    }

    /// Return a normalized quaternion, or identity when the input is too small.
    #[must_use]
    pub fn normalized(self) -> Self {
        let len = (self.x.mul_add(
            self.x,
            self.y
                .mul_add(self.y, self.z.mul_add(self.z, self.w * self.w)),
        ))
        .sqrt();
        if len <= f32::EPSILON {
            Self::IDENTITY
        } else {
            Self::new(self.x / len, self.y / len, self.z / len, self.w / len)
        }
    }

    /// Spherical interpolation for rotations.
    #[must_use]
    pub fn slerp(self, mut rhs: Self, t: f32) -> Self {
        let mut cos_theta = self.x.mul_add(
            rhs.x,
            self.y.mul_add(rhs.y, self.z.mul_add(rhs.z, self.w * rhs.w)),
        );
        if cos_theta < 0.0 {
            rhs = Self::new(-rhs.x, -rhs.y, -rhs.z, -rhs.w);
            cos_theta = -cos_theta;
        }
        if cos_theta > 0.9995 {
            return Self::new(
                lerp(self.x, rhs.x, t),
                lerp(self.y, rhs.y, t),
                lerp(self.z, rhs.z, t),
                lerp(self.w, rhs.w, t),
            )
            .normalized();
        }
        let theta = cos_theta.clamp(-1.0, 1.0).acos();
        let sin_theta = theta.sin();
        if sin_theta.abs() <= f32::EPSILON {
            return self;
        }
        let a = ((1.0 - t) * theta).sin() / sin_theta;
        let b = (t * theta).sin() / sin_theta;
        Self::new(
            self.x.mul_add(a, rhs.x * b),
            self.y.mul_add(a, rhs.y * b),
            self.z.mul_add(a, rhs.z * b),
            self.w.mul_add(a, rhs.w * b),
        )
        .normalized()
    }
}

impl Default for Quaternion {
    fn default() -> Self {
        Self::IDENTITY
    }
}

/// Translation, rotation, and scale for a scene node.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct NodeTransform {
    /// Local translation.
    pub translation: Vec3,
    /// Local rotation.
    pub rotation: Quaternion,
    /// Local non-uniform scale.
    pub scale: Vec3,
}

impl NodeTransform {
    /// Identity transform.
    pub const IDENTITY: Self = Self {
        translation: Vec3::new(0.0, 0.0, 0.0),
        rotation: Quaternion::IDENTITY,
        scale: Vec3::new(1.0, 1.0, 1.0),
    };

    /// Create a node transform.
    #[must_use]
    pub const fn new(translation: Vec3, rotation: Quaternion, scale: Vec3) -> Self {
        Self {
            translation,
            rotation,
            scale,
        }
    }

    /// Convert to a column-major transform matrix matching glTF layout.
    #[must_use]
    pub fn matrix(self) -> [[f32; 4]; 4] {
        let q = self.rotation.normalized();
        let x2 = q.x + q.x;
        let y2 = q.y + q.y;
        let z2 = q.z + q.z;
        let xx = q.x * x2;
        let xy = q.x * y2;
        let xz = q.x * z2;
        let yy = q.y * y2;
        let yz = q.y * z2;
        let zz = q.z * z2;
        let wx = q.w * x2;
        let wy = q.w * y2;
        let wz = q.w * z2;
        let sx = self.scale.x;
        let sy = self.scale.y;
        let sz = self.scale.z;
        [
            [(1.0 - (yy + zz)) * sx, (xy + wz) * sx, (xz - wy) * sx, 0.0],
            [(xy - wz) * sy, (1.0 - (xx + zz)) * sy, (yz + wx) * sy, 0.0],
            [(xz + wy) * sz, (yz - wx) * sz, (1.0 - (xx + yy)) * sz, 0.0],
            [
                self.translation.x,
                self.translation.y,
                self.translation.z,
                1.0,
            ],
        ]
    }
}

impl Default for NodeTransform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

/// Animation interpolation mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Interpolation {
    /// Hold the previous keyframe until the next one.
    Step,
    /// Interpolate linearly between keyframes. Rotations use slerp.
    Linear,
}

/// Node property targeted by an animation channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum AnimatedProperty {
    /// Node translation.
    Translation,
    /// Node rotation.
    Rotation,
    /// Node scale.
    Scale,
}

/// Sampled value stored in an animation sampler.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum AnimationValue {
    /// Vector value for translation or scale channels.
    Vec3(Vec3),
    /// Quaternion value for rotation channels.
    Rotation(Quaternion),
}

/// Keyframe sampler for one animation channel.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AnimationSampler {
    /// Keyframe input times in seconds.
    pub inputs: Vec<f32>,
    /// Output values aligned with [`Self::inputs`].
    pub outputs: Vec<AnimationValue>,
    /// Interpolation mode.
    pub interpolation: Interpolation,
}

impl AnimationSampler {
    /// Sample this sampler at `time_seconds`.
    #[must_use]
    pub fn sample(&self, time_seconds: f32) -> Option<AnimationValue> {
        if self.inputs.is_empty() || self.outputs.is_empty() {
            return None;
        }
        let count = self.inputs.len().min(self.outputs.len());
        if count == 0 {
            return None;
        }
        if count == 1 || time_seconds <= self.inputs[0] {
            return self.outputs.first().copied();
        }
        for index in 1..count {
            let next_time = self.inputs[index];
            if time_seconds <= next_time {
                let previous_time = self.inputs[index - 1];
                let previous = self.outputs[index - 1];
                let next = self.outputs[index];
                if self.interpolation == Interpolation::Step {
                    return Some(previous);
                }
                let span = (next_time - previous_time).max(f32::EPSILON);
                let t = ((time_seconds - previous_time) / span).clamp(0.0, 1.0);
                return interpolate_value(previous, next, t);
            }
        }
        self.outputs.get(count - 1).copied()
    }
}

/// One channel targeting a node TRS property.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AnimationChannel {
    /// glTF node index targeted by this channel.
    pub target_node: usize,
    /// Targeted node property.
    pub property: AnimatedProperty,
    /// Keyframe sampler.
    pub sampler: AnimationSampler,
}

/// A loaded animation clip.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AnimationClip {
    /// Display name.
    pub name: String,
    /// Clip duration in seconds.
    pub duration_seconds: f32,
    /// Animation channels in this clip.
    pub channels: Vec<AnimationChannel>,
}

impl AnimationClip {
    /// Number of imported channels.
    #[must_use]
    pub fn channel_count(&self) -> usize {
        self.channels.len()
    }
}

/// Inclusive start plus length for vertices or normals owned by a scene node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MeshRange {
    /// First element in the mesh buffer.
    pub start: usize,
    /// Number of elements in the range.
    pub len: usize,
}

impl MeshRange {
    /// Create a range.
    #[must_use]
    pub const fn new(start: usize, len: usize) -> Self {
        Self { start, len }
    }
}

/// Scene node metadata used to apply sampled animation to flattened mesh data.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AnimationNode {
    /// Source node index from the glTF document.
    pub index: usize,
    /// Optional source node name.
    pub name: Option<String>,
    /// Parent source node index, when this node is part of a hierarchy.
    pub parent: Option<usize>,
    /// Base node transform.
    pub base_transform: NodeTransform,
    /// Vertex ranges owned by this node.
    pub vertex_ranges: Vec<MeshRange>,
    /// Normal ranges owned by this node.
    pub normal_ranges: Vec<MeshRange>,
}

/// Per-vertex skinning data from glTF `JOINTS_0` and `WEIGHTS_0`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SkinnedVertex {
    /// Local joint indices inside [`SkinBinding::joint_nodes`].
    pub joints: [usize; 4],
    /// Joint weights.
    pub weights: [f32; 4],
}

/// Skin binding metadata for a flattened primitive range.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SkinBinding {
    /// Vertex range affected by this skin binding.
    pub vertex_range: MeshRange,
    /// Normal range affected by this skin binding.
    pub normal_range: Option<MeshRange>,
    /// glTF node indices used as joints.
    pub joint_nodes: Vec<usize>,
    /// Inverse bind matrices aligned with [`Self::joint_nodes`].
    pub inverse_bind_matrices: Vec<[[f32; 4]; 4]>,
    /// Per-vertex joint influences aligned with [`Self::vertex_range`].
    pub vertices: Vec<SkinnedVertex>,
}

/// Return `time_seconds` wrapped or clamped to the clip duration.
#[must_use]
pub fn playback_time(time_seconds: f32, duration_seconds: f32, looping: bool) -> f32 {
    if !time_seconds.is_finite() || duration_seconds <= f32::EPSILON {
        return 0.0;
    }
    if looping {
        time_seconds.rem_euclid(duration_seconds)
    } else {
        time_seconds.clamp(0.0, duration_seconds)
    }
}

/// Return a transformed copy of `mesh` for a sampled animation pose.
#[must_use]
pub fn sample_mesh_animation(
    mesh: &Mesh,
    clip_index: usize,
    time_seconds: f32,
    looping: bool,
) -> Option<Mesh> {
    let clip = mesh.animations.get(clip_index)?;
    if clip.channels.is_empty() || mesh.animation_nodes.is_empty() {
        return None;
    }

    let time = playback_time(time_seconds, clip.duration_seconds, looping);
    let mut sampled_nodes = mesh
        .animation_nodes
        .iter()
        .map(|node| (node.index, node.base_transform))
        .collect::<Vec<_>>();

    for channel in &clip.channels {
        let Some((_, transform)) = sampled_nodes
            .iter_mut()
            .find(|(index, _)| *index == channel.target_node)
        else {
            continue;
        };
        let Some(value) = channel.sampler.sample(time) else {
            continue;
        };
        match (channel.property, value) {
            (AnimatedProperty::Translation, AnimationValue::Vec3(value)) => {
                transform.translation = value;
            }
            (AnimatedProperty::Scale, AnimationValue::Vec3(value)) => {
                transform.scale = value;
            }
            (AnimatedProperty::Rotation, AnimationValue::Rotation(value)) => {
                transform.rotation = value;
            }
            _ => {}
        }
    }

    let global_matrices = mesh
        .animation_nodes
        .iter()
        .map(|node| {
            (
                node.index,
                global_matrix_for_node(node.index, &mesh.animation_nodes, &sampled_nodes),
            )
        })
        .collect::<Vec<_>>();

    let mut out = mesh.clone();
    for node in &mesh.animation_nodes {
        let matrix = global_matrices
            .iter()
            .find(|(index, _)| *index == node.index)
            .map(|(_, matrix)| *matrix)
            .unwrap_or_else(identity_matrix);
        for range in &node.vertex_ranges {
            for offset in 0..range.len {
                let idx = range.start + offset;
                if let (Some(vertex), Some(bind)) =
                    (out.vertices.get_mut(idx), mesh.bind_vertices.get(idx))
                {
                    *vertex = transform_point(matrix, *bind);
                }
            }
        }
        for range in &node.normal_ranges {
            for offset in 0..range.len {
                let idx = range.start + offset;
                if let (Some(normal), Some(bind)) =
                    (out.normals.get_mut(idx), mesh.bind_normals.get(idx))
                {
                    *normal = transform_normal(matrix, *bind);
                }
            }
        }
    }

    for skin in &mesh.skins {
        let joint_matrices = skin
            .joint_nodes
            .iter()
            .enumerate()
            .map(|(joint_index, node_index)| {
                let joint_global = global_matrices
                    .iter()
                    .find(|(index, _)| index == node_index)
                    .map(|(_, matrix)| *matrix)
                    .unwrap_or_else(identity_matrix);
                multiply_matrix(
                    joint_global,
                    skin.inverse_bind_matrices
                        .get(joint_index)
                        .copied()
                        .unwrap_or_else(identity_matrix),
                )
            })
            .collect::<Vec<_>>();

        for offset in 0..skin.vertex_range.len {
            let idx = skin.vertex_range.start + offset;
            let Some(influences) = skin.vertices.get(offset) else {
                continue;
            };
            let Some(bind) = mesh.bind_vertices.get(idx).copied() else {
                continue;
            };
            if let Some(vertex) = out.vertices.get_mut(idx) {
                *vertex = skin_point(bind, influences, &joint_matrices);
            }
            if let Some(normal_range) = skin.normal_range {
                let normal_idx = normal_range.start + offset;
                if let (Some(normal), Some(bind_normal)) = (
                    out.normals.get_mut(normal_idx),
                    mesh.bind_normals.get(normal_idx).copied(),
                ) {
                    *normal = skin_normal(bind_normal, influences, &joint_matrices);
                }
            }
        }
    }

    refresh_face_normals(&mut out.faces, &out.vertices, &out.normals);
    out.bounds = Bounds::from_vertices(&out.vertices).unwrap_or(out.bounds);
    Some(out)
}

fn global_matrix_for_node(
    node_index: usize,
    nodes: &[AnimationNode],
    sampled_nodes: &[(usize, NodeTransform)],
) -> [[f32; 4]; 4] {
    global_matrix_for_node_inner(node_index, nodes, sampled_nodes, 0)
}

fn global_matrix_for_node_inner(
    node_index: usize,
    nodes: &[AnimationNode],
    sampled_nodes: &[(usize, NodeTransform)],
    depth: usize,
) -> [[f32; 4]; 4] {
    if depth > nodes.len() {
        return identity_matrix();
    }
    let Some(node) = nodes.iter().find(|node| node.index == node_index) else {
        return identity_matrix();
    };
    let local = sampled_nodes
        .iter()
        .find(|(index, _)| *index == node_index)
        .map(|(_, transform)| transform.matrix())
        .unwrap_or_else(|| node.base_transform.matrix());
    if let Some(parent) = node.parent {
        multiply_matrix(
            global_matrix_for_node_inner(parent, nodes, sampled_nodes, depth + 1),
            local,
        )
    } else {
        local
    }
}

fn skin_point(bind: Vec3, influences: &SkinnedVertex, joint_matrices: &[[[f32; 4]; 4]]) -> Vec3 {
    let mut out = Vec3::default();
    let mut total = 0.0_f32;
    for i in 0..4 {
        let weight = influences.weights[i];
        if weight <= f32::EPSILON {
            continue;
        }
        let Some(matrix) = joint_matrices.get(influences.joints[i]) else {
            continue;
        };
        out += transform_point(*matrix, bind) * weight;
        total += weight;
    }
    if total > f32::EPSILON {
        out / total
    } else {
        bind
    }
}

fn skin_normal(bind: Vec3, influences: &SkinnedVertex, joint_matrices: &[[[f32; 4]; 4]]) -> Vec3 {
    let mut out = Vec3::default();
    let mut total = 0.0_f32;
    for i in 0..4 {
        let weight = influences.weights[i];
        if weight <= f32::EPSILON {
            continue;
        }
        let Some(matrix) = joint_matrices.get(influences.joints[i]) else {
            continue;
        };
        out += transform_normal(*matrix, bind) * weight;
        total += weight;
    }
    if total > f32::EPSILON {
        (out / total).normalized()
    } else {
        bind.normalized()
    }
}

fn interpolate_value(
    previous: AnimationValue,
    next: AnimationValue,
    t: f32,
) -> Option<AnimationValue> {
    match (previous, next) {
        (AnimationValue::Vec3(a), AnimationValue::Vec3(b)) => Some(AnimationValue::Vec3(
            Vec3::new(lerp(a.x, b.x, t), lerp(a.y, b.y, t), lerp(a.z, b.z, t)),
        )),
        (AnimationValue::Rotation(a), AnimationValue::Rotation(b)) => {
            Some(AnimationValue::Rotation(a.slerp(b, t)))
        }
        _ => None,
    }
}

fn refresh_face_normals(faces: &mut [Face], vertices: &[Vec3], normals: &[Vec3]) {
    for face in faces {
        face.normal = face
            .normal_indices
            .iter()
            .flatten()
            .next()
            .and_then(|&idx| normals.get(idx).copied())
            .or_else(|| {
                let [a, b, c] = first_three(face)?;
                let a = *vertices.get(a)?;
                let b = *vertices.get(b)?;
                let c = *vertices.get(c)?;
                Some((b - a).cross(c - a).normalized())
            });
    }
}

fn first_three(face: &Face) -> Option<[usize; 3]> {
    Some([
        *face.indices.first()?,
        *face.indices.get(1)?,
        *face.indices.get(2)?,
    ])
}

/// Transform a point by a column-major matrix.
#[must_use]
pub fn transform_point(matrix: [[f32; 4]; 4], point: Vec3) -> Vec3 {
    Vec3::new(
        matrix[0][0].mul_add(
            point.x,
            matrix[1][0].mul_add(point.y, matrix[2][0] * point.z),
        ) + matrix[3][0],
        matrix[0][1].mul_add(
            point.x,
            matrix[1][1].mul_add(point.y, matrix[2][1] * point.z),
        ) + matrix[3][1],
        matrix[0][2].mul_add(
            point.x,
            matrix[1][2].mul_add(point.y, matrix[2][2] * point.z),
        ) + matrix[3][2],
    )
}

/// Return an identity 4x4 matrix.
#[must_use]
pub const fn identity_matrix() -> [[f32; 4]; 4] {
    [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

/// Multiply two column-major 4x4 matrices.
#[must_use]
pub fn multiply_matrix(a: [[f32; 4]; 4], b: [[f32; 4]; 4]) -> [[f32; 4]; 4] {
    let mut out = [[0.0; 4]; 4];
    for col in 0..4 {
        for row in 0..4 {
            out[col][row] = a[0][row].mul_add(
                b[col][0],
                a[1][row].mul_add(
                    b[col][1],
                    a[2][row].mul_add(b[col][2], a[3][row] * b[col][3]),
                ),
            );
        }
    }
    out
}

/// Transform a normal by a column-major matrix.
#[must_use]
pub fn transform_normal(matrix: [[f32; 4]; 4], normal: Vec3) -> Vec3 {
    Vec3::new(
        matrix[0][0].mul_add(
            normal.x,
            matrix[1][0].mul_add(normal.y, matrix[2][0] * normal.z),
        ),
        matrix[0][1].mul_add(
            normal.x,
            matrix[1][1].mul_add(normal.y, matrix[2][1] * normal.z),
        ),
        matrix[0][2].mul_add(
            normal.x,
            matrix[1][2].mul_add(normal.y, matrix[2][2] * normal.z),
        ),
    )
    .normalized()
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vec3_sampler(interpolation: Interpolation) -> AnimationSampler {
        AnimationSampler {
            inputs: vec![0.0, 1.0],
            outputs: vec![
                AnimationValue::Vec3(Vec3::new(0.0, 0.0, 0.0)),
                AnimationValue::Vec3(Vec3::new(10.0, 0.0, 0.0)),
            ],
            interpolation,
        }
    }

    #[test]
    fn samples_linear_vec3() {
        assert_eq!(
            vec3_sampler(Interpolation::Linear).sample(0.25),
            Some(AnimationValue::Vec3(Vec3::new(2.5, 0.0, 0.0)))
        );
    }

    #[test]
    fn samples_step_vec3() {
        assert_eq!(
            vec3_sampler(Interpolation::Step).sample(0.75),
            Some(AnimationValue::Vec3(Vec3::new(0.0, 0.0, 0.0)))
        );
    }

    #[test]
    fn normalizes_playback_time() {
        assert_eq!(playback_time(2.5, 1.0, true), 0.5);
        assert_eq!(playback_time(2.5, 1.0, false), 1.0);
    }

    #[test]
    fn sampled_translation_moves_bound_vertices() {
        let mut mesh = Mesh::new(
            "tri",
            vec![
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
            ],
            vec![Face::new(vec![0, 1, 2])],
            vec![],
        )
        .unwrap();
        mesh.bind_vertices = mesh.vertices.clone();
        mesh.animation_nodes.push(AnimationNode {
            index: 0,
            name: None,
            parent: None,
            base_transform: NodeTransform::default(),
            vertex_ranges: vec![MeshRange::new(0, 3)],
            normal_ranges: Vec::new(),
        });
        mesh.animations.push(AnimationClip {
            name: "move".into(),
            duration_seconds: 1.0,
            channels: vec![AnimationChannel {
                target_node: 0,
                property: AnimatedProperty::Translation,
                sampler: vec3_sampler(Interpolation::Linear),
            }],
        });

        let sampled = sample_mesh_animation(&mesh, 0, 0.5, true).unwrap();
        assert_eq!(sampled.vertices[0], Vec3::new(5.0, 0.0, 0.0));
    }

    #[test]
    fn sampled_joint_animation_skins_vertices() {
        let mut mesh = Mesh::new(
            "skinned-tri",
            vec![
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
            ],
            vec![Face::new(vec![0, 1, 2])],
            vec![],
        )
        .unwrap();
        mesh.bind_vertices = mesh.vertices.clone();
        mesh.animation_nodes.push(AnimationNode {
            index: 0,
            name: Some("mesh".into()),
            parent: None,
            base_transform: NodeTransform::default(),
            vertex_ranges: vec![MeshRange::new(0, 3)],
            normal_ranges: Vec::new(),
        });
        mesh.animation_nodes.push(AnimationNode {
            index: 1,
            name: Some("joint".into()),
            parent: None,
            base_transform: NodeTransform::default(),
            vertex_ranges: Vec::new(),
            normal_ranges: Vec::new(),
        });
        mesh.skins.push(SkinBinding {
            vertex_range: MeshRange::new(0, 3),
            normal_range: None,
            joint_nodes: vec![1],
            inverse_bind_matrices: vec![identity_matrix()],
            vertices: vec![
                SkinnedVertex {
                    joints: [0, 0, 0, 0],
                    weights: [1.0, 0.0, 0.0, 0.0],
                };
                3
            ],
        });
        mesh.animations.push(AnimationClip {
            name: "joint-move".into(),
            duration_seconds: 1.0,
            channels: vec![AnimationChannel {
                target_node: 1,
                property: AnimatedProperty::Translation,
                sampler: vec3_sampler(Interpolation::Linear),
            }],
        });

        let sampled = sample_mesh_animation(&mesh, 0, 0.5, false).unwrap();
        assert_eq!(sampled.vertices[0], Vec3::new(5.0, 0.0, 0.0));
        assert_eq!(sampled.vertices[1], Vec3::new(6.0, 0.0, 0.0));
    }
}
