use glam::{Mat4, Quat, Vec3};
use io_types::{Bounds, Vec3 as Position};
use std::path::Path;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Vertex {
    pub position: Position,
    pub normal: Position,
    pub color: [f32; 4],
    pub emission: [f32; 3],
    pub joints: [u32; 4],
    pub weights: [f32; 4],
}

#[derive(Clone)]
struct Node {
    name: Option<String>,
    parent: Option<usize>,
    translation: Vec3,
    rotation: Quat,
    scale: Vec3,
}
enum Values {
    Translation(Vec<Vec3>),
    Rotation(Vec<Quat>),
    Scale(Vec<Vec3>),
}
struct Track {
    node: usize,
    times: Vec<f32>,
    values: Values,
    step: bool,
}
pub struct Clip {
    pub(crate) name: String,
    pub(crate) duration: f32,
    start: f32,
    tracks: Vec<Track>,
}
impl Clip {
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn duration(&self) -> f32 {
        self.duration
    }
}
struct Joint {
    node: usize,
    inverse_bind: Mat4,
}
pub struct Model {
    pub(crate) vertices: Vec<Vertex>,
    pub(crate) indices: Vec<u32>,
    pub(crate) bounds: Bounds,
    pub(crate) clips: Vec<Clip>,
    nodes: Vec<Node>,
    order: Vec<usize>,
    joints: Vec<Joint>,
}

fn position(v: Vec3) -> Position {
    Position::new(v.x, v.y, v.z)
}
fn conversion() -> Mat4 {
    Mat4::from_rotation_x(std::f32::consts::FRAC_PI_2)
}

impl Model {
    pub fn vertices(&self) -> &[Vertex] {
        &self.vertices
    }
    pub fn indices(&self) -> &[u32] {
        &self.indices
    }
    pub fn bounds(&self) -> Bounds {
        self.bounds
    }
    pub fn clips(&self) -> &[Clip] {
        &self.clips
    }
    pub fn box_with_bounds(bounds: Bounds) -> Result<Self, String> {
        if !bounds.valid() {
            return Err("invalid box bounds".into());
        }
        let mut model = Self::unit_box();
        let size = bounds.max - bounds.min;
        if !size.finite() {
            return Err("box dimensions overflow".into());
        }
        for v in &mut model.vertices {
            v.position = bounds.min
                + Position::new(
                    v.position.x * size.x,
                    v.position.y * size.y,
                    v.position.z * size.z,
                );
        }
        model.bounds = bounds;
        Ok(model)
    }
    pub fn joint_count(&self) -> usize {
        self.joints.len()
    }

    /// Map this mesh's palette slots to a canonical animation source. Geometry
    /// exports may reorder joints, but must preserve named ancestry and bind space.
    pub fn palette_mapping_from(&self, source: &Self) -> Result<Vec<usize>, String> {
        let mut names = std::collections::HashMap::new();
        for (index, joint) in source.joints.iter().enumerate() {
            let name = source.nodes[joint.node]
                .name
                .as_deref()
                .filter(|n| !n.is_empty())
                .ok_or("animation source has unnamed joints")?;
            if names.insert(name, index).is_some() {
                return Err(format!("animation source has duplicate joint name: {name}"));
            }
        }
        let mut seen = std::collections::HashSet::new();
        self.joints
            .iter()
            .map(|joint| {
                let name = self.nodes[joint.node]
                    .name
                    .as_deref()
                    .filter(|n| !n.is_empty())
                    .ok_or("variant has unnamed joints")?;
                if !seen.insert(name) {
                    return Err(format!("variant has duplicate joint name: {name}"));
                }
                let &index = names.get(name).ok_or_else(|| {
                    format!("variant joint {name} is absent from animation source")
                })?;
                let canonical = &source.joints[index];
                if !joint
                    .inverse_bind
                    .abs_diff_eq(canonical.inverse_bind, 0.0001)
                {
                    return Err(format!(
                        "variant joint {name} has incompatible inverse bind"
                    ));
                }
                let (mut a, mut b) = (Some(joint.node), Some(canonical.node));
                while let (Some(i), Some(j)) = (a, b) {
                    let (v, s) = (&self.nodes[i], &source.nodes[j]);
                    let vm =
                        Mat4::from_scale_rotation_translation(v.scale, v.rotation, v.translation);
                    let sm =
                        Mat4::from_scale_rotation_translation(s.scale, s.rotation, s.translation);
                    if v.name != s.name || !vm.abs_diff_eq(sm, 0.0001) {
                        return Err(format!(
                            "variant joint {name} has incompatible ancestry/rest transform"
                        ));
                    }
                    (a, b) = (v.parent, s.parent);
                }
                if a.is_some() || b.is_some() {
                    return Err(format!("variant joint {name} has incompatible hierarchy"));
                }
                Ok(index)
            })
            .collect()
    }

    /// Conservative geometry envelope under the source's animations, including
    /// variants exported without clips. Mapping comes from palette_mapping_from.
    pub fn bounds_with_source(&self, source: &Self, mapping: &[usize]) -> Result<Bounds, String> {
        if mapping.len() != self.joints.len() || mapping.iter().any(|&i| i >= source.joints.len()) {
            return Err("invalid palette mapping for bounds calculation".into());
        }
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        let mut translation: Vec<_> = source
            .nodes
            .iter()
            .map(|n| n.translation.length())
            .collect();
        let mut scale: Vec<_> = source
            .nodes
            .iter()
            .map(|n| n.scale.abs().max_element())
            .collect();
        for clip in &source.clips {
            for track in &clip.tracks {
                match &track.values {
                    Values::Translation(v) => {
                        for v in v {
                            translation[track.node] = translation[track.node].max(v.length());
                        }
                    }
                    Values::Scale(v) => {
                        for v in v {
                            scale[track.node] = scale[track.node].max(v.abs().max_element());
                        }
                    }
                    _ => {}
                }
            }
        }
        for &i in &source.order {
            if let Some(parent) = source.nodes[i].parent {
                translation[i] = translation[parent] + scale[parent] * translation[i];
                scale[i] *= scale[parent];
            }
        }
        for vertex in &self.vertices {
            let p = Vec3::new(vertex.position.x, vertex.position.y, vertex.position.z);
            if vertex.weights.iter().sum::<f32>() > 0. {
                let radius = (0..4)
                    .filter(|&k| vertex.weights[k] > 0.)
                    .map(|k| {
                        let joint = &source.joints[mapping[vertex.joints[k] as usize]];
                        translation[joint.node]
                            + scale[joint.node] * joint.inverse_bind.transform_point3(p).length()
                    })
                    .fold(0_f32, f32::max);
                min = min.min(Vec3::splat(-radius));
                max = max.max(Vec3::splat(radius));
            } else {
                min = min.min(p);
                max = max.max(p);
            }
        }
        let bounds = Bounds {
            min: position(min),
            max: position(max),
        };
        if !bounds.valid() {
            return Err("geometry or animation bounds overflow".into());
        }
        Ok(bounds)
    }

    fn globals(&self, clip: Option<usize>, time: f32) -> Vec<Mat4> {
        self.globals_sample(clip, time, true)
    }

    fn globals_sample(&self, clip: Option<usize>, time: f32, looping: bool) -> Vec<Mat4> {
        let mut nodes = self.nodes.clone();
        if let Some(clip) = clip.and_then(|id| self.clips.get(id)) {
            let t = clip.start
                + if time.is_finite() && clip.duration > 0. {
                    if looping {
                        time.rem_euclid(clip.duration)
                    } else {
                        time.clamp(0., clip.duration)
                    }
                } else {
                    0.
                };
            for track in &clip.tracks {
                let upper = track.times.partition_point(|&key| key <= t);
                let next = upper.min(track.times.len() - 1);
                let previous = upper.saturating_sub(1);
                let alpha = if track.step || previous == next {
                    0.
                } else {
                    ((t - track.times[previous]) / (track.times[next] - track.times[previous]))
                        .clamp(0., 1.)
                };
                let node = &mut nodes[track.node];
                match &track.values {
                    Values::Translation(v) => node.translation = v[previous].lerp(v[next], alpha),
                    Values::Scale(v) => node.scale = v[previous].lerp(v[next], alpha),
                    Values::Rotation(v) => {
                        node.rotation = v[previous].slerp(v[next], alpha).normalize()
                    }
                }
            }
        }
        let mut globals = vec![Mat4::IDENTITY; nodes.len()];
        for &i in &self.order {
            let n = &nodes[i];
            let local = Mat4::from_scale_rotation_translation(n.scale, n.rotation, n.translation);
            globals[i] = n.parent.map_or(local, |parent| globals[parent] * local);
        }
        globals
    }

    pub fn palette(&self, clip: Option<usize>, time: f32) -> Vec<[f32; 16]> {
        self.palette_sample(clip, time, true)
    }

    pub fn palette_clamped(&self, clip: Option<usize>, time: f32) -> Vec<[f32; 16]> {
        self.palette_sample(clip, time, false)
    }

    fn palette_sample(&self, clip: Option<usize>, time: f32, looping: bool) -> Vec<[f32; 16]> {
        if self.joints.is_empty() {
            return Vec::new();
        }
        let globals = self.globals_sample(clip, time, looping);
        self.joints
            .iter()
            .map(|j| (conversion() * globals[j.node] * j.inverse_bind).to_cols_array())
            .collect()
    }

    pub fn unit_box() -> Self {
        Self {
            vertices: super::BOX_VERTICES
                .iter()
                .map(|&p| Vertex {
                    position: p,
                    normal: Position::default(),
                    color: [1.; 4],
                    emission: [0.; 3],
                    joints: [0; 4],
                    weights: [0.; 4],
                })
                .collect(),
            indices: super::BOX_TRIANGLES.to_vec(),
            bounds: Bounds {
                min: Position::default(),
                max: Position::new(1., 1., 1.),
            },
            clips: Vec::new(),
            nodes: Vec::new(),
            order: Vec::new(),
            joints: Vec::new(),
        }
    }
}

/// Preserve authored bind space. Static vertices are baked to Z-up; skin palettes
/// convert skinned vertices to Z-up after joint deformation, without normalization.
pub fn load_model(path: &Path) -> Result<Model, String> {
    let (document, buffers, _) =
        gltf::import(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let fail = |message: &str| format!("{}: {message}", path.display());
    let mut model = Model {
        vertices: Vec::new(),
        indices: Vec::new(),
        clips: Vec::new(),
        bounds: Bounds {
            min: Position::default(),
            max: Position::default(),
        },
        nodes: document
            .nodes()
            .map(|n| {
                let (t, r, s) = n.transform().decomposed();
                Node {
                    name: n.name().map(str::to_owned),
                    parent: None,
                    translation: Vec3::from(t),
                    rotation: Quat::from_array(r),
                    scale: Vec3::from(s),
                }
            })
            .collect(),
        order: Vec::new(),
        joints: Vec::new(),
    };
    for node in document.nodes() {
        for child in node.children() {
            if model.nodes[child.index()]
                .parent
                .replace(node.index())
                .is_some()
            {
                return Err(fail("node has multiple parents"));
            }
        }
    }
    // Topological evaluation supports files whose node indices are not parent-first.
    let mut pending: Vec<_> = document
        .nodes()
        .filter(|n| model.nodes[n.index()].parent.is_none())
        .map(|n| n.index())
        .collect();
    let child_lists: Vec<Vec<_>> = document
        .nodes()
        .map(|n| n.children().map(|c| c.index()).collect())
        .collect();
    while let Some(i) = pending.pop() {
        model.order.push(i);
        pending.extend(&child_lists[i]);
    }
    if model.order.len() != model.nodes.len() {
        return Err(fail("cyclic node hierarchy"));
    }
    for node in &model.nodes {
        if !node.translation.is_finite()
            || !node.scale.is_finite()
            || node.scale.abs().min_element() == 0.
            || !node.rotation.is_finite()
            || (node.rotation.length() - 1.).abs() > 0.001
        {
            return Err(fail("invalid node transform"));
        }
    }
    let mut skin_offsets = Vec::new();
    let mut skin_counts = Vec::new();
    for skin in document.skins() {
        skin_offsets.push(model.joints.len() as u32);
        let joints: Vec<_> = skin.joints().collect();
        skin_counts.push(joints.len());
        let reader = skin.reader(|b| buffers.get(b.index()).map(|d| d.as_ref()));
        let binds: Vec<_> = reader
            .read_inverse_bind_matrices()
            .map(|r| r.map(|m| Mat4::from_cols_array_2d(&m)).collect())
            .unwrap_or_else(|| vec![Mat4::IDENTITY; joints.len()]);
        if binds.len() != joints.len() || binds.iter().any(|m| !m.is_finite()) {
            return Err(fail("invalid inverse bind matrices"));
        }
        model
            .joints
            .extend(joints.iter().zip(binds).map(|(n, inverse_bind)| Joint {
                node: n.index(),
                inverse_bind,
            }));
    }
    let globals = model.globals(None, 0.);
    let scene = document
        .default_scene()
        .or_else(|| document.scenes().next())
        .ok_or_else(|| fail("missing scene"))?;
    let mut visible = vec![false; model.nodes.len()];
    let mut stack: Vec<_> = scene.nodes().collect();
    while let Some(n) = stack.pop() {
        visible[n.index()] = true;
        stack.extend(n.children());
    }
    for node in document.nodes().filter(|n| visible[n.index()]) {
        let Some(mesh) = node.mesh() else { continue };
        let matrix = conversion() * globals[node.index()];
        let normal_matrix = matrix.inverse().transpose();
        for primitive in mesh.primitives() {
            if primitive.mode() != gltf::mesh::Mode::Triangles {
                return Err(fail("only triangle primitives are supported"));
            }
            let reader = primitive.reader(|b| buffers.get(b.index()).map(|d| d.as_ref()));
            let positions: Vec<_> = reader
                .read_positions()
                .ok_or_else(|| fail("missing positions"))?
                .map(Vec3::from)
                .collect();
            let normals: Vec<_> = reader
                .read_normals()
                .map(|r| r.map(Vec3::from).collect())
                .unwrap_or_else(|| vec![Vec3::ZERO; positions.len()]);
            let indices: Vec<_> = reader
                .read_indices()
                .map(|r| r.into_u32().collect())
                .unwrap_or_else(|| (0..positions.len() as u32).collect());
            let colors: Vec<_> = reader
                .read_colors(0)
                .map(|r| r.into_rgba_f32().collect())
                .unwrap_or_else(|| vec![[1.; 4]; positions.len()]);
            let (joints, weights): (Vec<[u16; 4]>, Vec<[f32; 4]>) = if node.skin().is_some() {
                (
                    reader
                        .read_joints(0)
                        .ok_or_else(|| fail("skin lacks joints"))?
                        .into_u16()
                        .collect(),
                    reader
                        .read_weights(0)
                        .ok_or_else(|| fail("skin lacks weights"))?
                        .into_f32()
                        .collect(),
                )
            } else {
                (
                    vec![[0; 4]; positions.len()],
                    vec![[0.; 4]; positions.len()],
                )
            };
            if [normals.len(), colors.len(), joints.len(), weights.len()]
                .iter()
                .any(|&n| n != positions.len())
                || indices.len() % 3 != 0
                || indices.iter().any(|&i| i as usize >= positions.len())
            {
                return Err(fail("invalid vertex attributes or indices"));
            }
            let material = primitive.material();
            if material.alpha_mode() != gltf::material::AlphaMode::Opaque
                || material
                    .pbr_metallic_roughness()
                    .base_color_texture()
                    .is_some()
                || material
                    .pbr_metallic_roughness()
                    .metallic_roughness_texture()
                    .is_some()
                || material.normal_texture().is_some()
                || material.occlusion_texture().is_some()
                || material.emissive_texture().is_some()
            {
                return Err(fail("this renderer supports opaque material factors, not textured/transparent materials yet"));
            }
            let base = material.pbr_metallic_roughness().base_color_factor();
            let offset =
                u32::try_from(model.vertices.len()).map_err(|_| fail("too many vertices"))?;
            offset
                .checked_add(u32::try_from(positions.len()).map_err(|_| fail("too many vertices"))?)
                .ok_or_else(|| fail("too many vertices"))?;
            for i in 0..positions.len() {
                let mut p = positions[i];
                let mut n = normals[i];
                let mut w = weights[i];
                let mut joint = [0; 4];
                if let Some(skin) = node.skin() {
                    if w.iter().any(|v| !v.is_finite() || *v < 0.) || w.iter().sum::<f32>() <= 0. {
                        return Err(fail("invalid skin weights"));
                    }
                    let sum = w.iter().sum::<f32>();
                    for k in 0..4 {
                        if joints[i][k] as usize >= skin_counts[skin.index()] {
                            return Err(fail("joint index out of bounds"));
                        }
                        joint[k] = skin_offsets[skin.index()] + u32::from(joints[i][k]);
                        w[k] /= sum;
                    }
                } else {
                    p = matrix.transform_point3(p);
                    n = normal_matrix.transform_vector3(n).normalize_or_zero();
                }
                if !p.is_finite()
                    || !n.is_finite()
                    || colors[i]
                        .iter()
                        .chain(base.iter())
                        .any(|v| !v.is_finite() || *v < 0.)
                    || material
                        .emissive_factor()
                        .iter()
                        .any(|v| !v.is_finite() || *v < 0.)
                {
                    return Err(fail("nonfinite geometry"));
                }
                model.vertices.push(Vertex {
                    position: position(p),
                    normal: position(n),
                    color: std::array::from_fn(|k| colors[i][k] * base[k]),
                    emission: material.emissive_factor(),
                    joints: joint,
                    weights: w,
                });
            }
            model
                .indices
                .extend(indices.into_iter().map(|i| i + offset));
        }
    }
    for animation in document.animations() {
        let mut clip = Clip {
            name: animation.name().unwrap_or("unnamed").into(),
            duration: 0.,
            start: f32::INFINITY,
            tracks: Vec::new(),
        };
        for channel in animation.channels() {
            let reader = channel.reader(|b| buffers.get(b.index()).map(|d| d.as_ref()));
            let times: Vec<_> = reader
                .read_inputs()
                .ok_or_else(|| fail("missing animation times"))?
                .collect();
            if times.is_empty()
                || times.iter().any(|t| !t.is_finite() || *t < 0.)
                || times.windows(2).any(|t| t[0] >= t[1])
            {
                return Err(fail("invalid animation times"));
            }
            use gltf::animation::util::ReadOutputs;
            let values = match reader
                .read_outputs()
                .ok_or_else(|| fail("missing animation values"))?
            {
                ReadOutputs::Translations(v) => Values::Translation(v.map(Vec3::from).collect()),
                ReadOutputs::Rotations(v) => {
                    Values::Rotation(v.into_f32().map(Quat::from_array).collect())
                }
                ReadOutputs::Scales(v) => Values::Scale(v.map(Vec3::from).collect()),
                _ => return Err(fail("morph animation is not supported")),
            };
            let (len, valid) = match &values {
                Values::Translation(v) => (v.len(), v.iter().all(|v| v.is_finite())),
                Values::Rotation(v) => (
                    v.len(),
                    v.iter()
                        .all(|v| v.is_finite() && (v.length() - 1.).abs() < 0.001),
                ),
                Values::Scale(v) => (
                    v.len(),
                    v.iter().all(|v| v.is_finite() && v.min_element() > 0.),
                ),
            };
            if channel.sampler().interpolation() == gltf::animation::Interpolation::CubicSpline {
                return Err(fail("export animation with linear or step interpolation"));
            }
            if len != times.len() || !valid {
                return Err(fail("invalid animation values"));
            }
            clip.duration = clip.duration.max(*times.last().unwrap());
            clip.start = clip.start.min(times[0]);
            clip.tracks.push(Track {
                node: channel.target().node().index(),
                times,
                values,
                step: channel.sampler().interpolation() == gltf::animation::Interpolation::Step,
            });
        }
        if clip.tracks.is_empty() {
            return Err(fail("empty animation"));
        }
        clip.duration -= clip.start;
        model.clips.push(clip);
    }
    if model.vertices.is_empty() {
        return Err(fail("empty model"));
    }
    if !model.clips.is_empty() && model.joints.is_empty() {
        return Err(fail("rigid-node animation is not implemented yet"));
    }
    model.bounds =
        model.bounds_with_source(&model, &(0..model.joint_count()).collect::<Vec<_>>())?;
    Ok(model)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn palette_mapping_handles_joint_order_and_rejects_incompatible_bind_space() {
        let mut source = Model::unit_box();
        source.nodes = vec![
            Node {
                name: Some("root".into()),
                parent: None,
                translation: Vec3::ZERO,
                rotation: Quat::IDENTITY,
                scale: Vec3::ONE,
            },
            Node {
                name: Some("arm".into()),
                parent: Some(0),
                translation: Vec3::X,
                rotation: Quat::IDENTITY,
                scale: Vec3::ONE,
            },
        ];
        source.order = vec![0, 1];
        source.joints = vec![
            Joint {
                node: 0,
                inverse_bind: Mat4::IDENTITY,
            },
            Joint {
                node: 1,
                inverse_bind: Mat4::from_translation(-Vec3::X),
            },
        ];
        let mut variant = Model::unit_box();
        variant.nodes = source.nodes.clone();
        variant.joints = vec![
            Joint {
                node: 1,
                inverse_bind: source.joints[1].inverse_bind,
            },
            Joint {
                node: 0,
                inverse_bind: Mat4::IDENTITY,
            },
        ];
        assert_eq!(variant.palette_mapping_from(&source).unwrap(), vec![1, 0]);
        variant.joints.pop();
        assert_eq!(variant.palette_mapping_from(&source).unwrap(), vec![1]);
        variant.joints[0].inverse_bind = Mat4::IDENTITY;
        assert!(variant
            .palette_mapping_from(&source)
            .unwrap_err()
            .contains("inverse bind"));
        variant.joints[0].inverse_bind = source.joints[1].inverse_bind;
        variant.nodes[1].translation = Vec3::Y;
        assert!(variant
            .palette_mapping_from(&source)
            .unwrap_err()
            .contains("rest transform"));
        variant.nodes[1] = source.nodes[1].clone();
        variant.nodes[1].name = Some("missing".into());
        assert!(variant
            .palette_mapping_from(&source)
            .unwrap_err()
            .contains("absent"));
    }
    #[test]
    fn step_tracks_hold_last_key_and_linear_rotation_uses_short_arc() {
        let mut model = Model::unit_box();
        model.nodes = vec![Node {
            name: Some("test".into()),
            parent: None,
            translation: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
        }];
        model.order = vec![0];
        model.clips.push(Clip {
            name: "test".into(),
            start: 2.,
            duration: 2.,
            tracks: vec![
                Track {
                    node: 0,
                    times: vec![2., 3.],
                    values: Values::Translation(vec![Vec3::ZERO, Vec3::X]),
                    step: true,
                },
                Track {
                    node: 0,
                    times: vec![2., 4.],
                    values: Values::Rotation(vec![Quat::IDENTITY, -Quat::IDENTITY]),
                    step: false,
                },
            ],
        });
        assert!(model.globals(Some(0), 0.5)[0]
            .transform_point3(Vec3::ZERO)
            .abs_diff_eq(Vec3::ZERO, 0.0001));
        assert!(model.globals(Some(0), 1.5)[0]
            .transform_point3(Vec3::ZERO)
            .abs_diff_eq(Vec3::X, 0.0001));
        assert!(model.globals(Some(0), 1.)[0]
            .transform_vector3(Vec3::Y)
            .abs_diff_eq(Vec3::Y, 0.0001));
    }
    #[test]
    fn runner_clip_moves_vertices_and_loops_in_bind_space() {
        let model = load_model(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/street-kit/runner.glb"),
        )
        .unwrap();
        assert_eq!(model.joint_count(), 14);
        let clip = model.clips.iter().position(|c| c.name == "Run").unwrap();
        let a = model.palette(Some(clip), 0.);
        let b = model.palette(Some(clip), 0.25);
        assert_eq!(a, model.palette(Some(clip), model.clips[clip].duration));
        assert_ne!(a, b);
        for time in [0., 0.125, 0.25, 0.5, 0.875] {
            let palette = model.palette(Some(clip), time);
            for vertex in &model.vertices {
                let p = Vec3::new(vertex.position.x, vertex.position.y, vertex.position.z);
                let posed = (0..4)
                    .map(|i| {
                        Mat4::from_cols_array(&palette[vertex.joints[i] as usize])
                            .transform_point3(p)
                            * vertex.weights[i]
                    })
                    .sum::<Vec3>();
                assert!(posed.is_finite());
                assert!(posed.z > -0.05 && posed.z < 2.1, "{posed:?}");
                assert!(posed.x >= model.bounds.min.x && posed.x <= model.bounds.max.x);
            }
        }
    }

    #[test]
    fn collapse_lowers_the_body_and_holds_its_final_pose() {
        let model = load_model(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/street-kit/runner.glb"),
        )
        .unwrap();
        let clip = model
            .clips
            .iter()
            .position(|c| c.name == "Collapse")
            .unwrap();
        let duration = model.clips[clip].duration;
        assert_eq!(duration, 1.5);
        let final_pose = model.palette_clamped(Some(clip), duration);
        assert_eq!(final_pose, model.palette_clamped(Some(clip), duration + 5.));
        assert_ne!(final_pose, model.palette_clamped(Some(clip), 0.));
        for step in 0..=36 {
            let palette = model.palette_clamped(Some(clip), step as f32 / 24.);
            let mut top = f32::NEG_INFINITY;
            for vertex in &model.vertices {
                let p = Vec3::new(vertex.position.x, vertex.position.y, vertex.position.z);
                let posed = (0..4)
                    .map(|i| {
                        Mat4::from_cols_array(&palette[vertex.joints[i] as usize])
                            .transform_point3(p)
                            * vertex.weights[i]
                    })
                    .sum::<Vec3>();
                assert!(
                    posed.is_finite() && posed.z > -0.02,
                    "step {step}: {posed:?}"
                );
                top = top.max(posed.z);
            }
            if step == 0 {
                assert!(top > 1.5);
            }
            if step == 36 {
                assert!(top < 0.65, "collapsed body is still {top} high");
            }
        }
    }
}
