#![forbid(unsafe_code)]
//! Reusable mesh data and glTF import, independent of placed items and cameras.
use io_types::Vec3;
use std::path::Path;
mod animated;
pub mod contract;
pub use animated::{load_model, Model, Vertex};

pub static BOX_VERTICES: [Vec3; 8] = [
    Vec3::new(0., 0., 0.),
    Vec3::new(1., 0., 0.),
    Vec3::new(1., 1., 0.),
    Vec3::new(0., 1., 0.),
    Vec3::new(0., 0., 1.),
    Vec3::new(1., 0., 1.),
    Vec3::new(1., 1., 1.),
    Vec3::new(0., 1., 1.),
];
pub static BOX_TRIANGLES: [u32; 36] = [
    0, 1, 2, 2, 3, 0, 4, 6, 5, 6, 4, 7, 0, 4, 5, 5, 1, 0, 1, 5, 6, 6, 2, 1, 2, 6, 7, 7, 3, 2, 4, 0,
    3, 3, 7, 4,
];

#[derive(Clone, Debug, Default)]
pub struct Mesh {
    pub vertices: Vec<Vec3>,
    pub normals: Vec<Vec3>,
    pub indices: Vec<u32>,
}

pub struct MeshView<'a> {
    pub vertices: &'a [Vec3],
    pub normals: &'a [Vec3],
    pub indices: &'a [u32],
}

type Matrix = [[f32; 4]; 4];
const IDENTITY: Matrix = [
    [1., 0., 0., 0.],
    [0., 1., 0., 0.],
    [0., 0., 1., 0.],
    [0., 0., 0., 1.],
];

fn compose(parent: Matrix, local: Matrix) -> Matrix {
    let mut result = [[0.; 4]; 4];
    for (column, values) in result.iter_mut().enumerate() {
        for (row, value) in values.iter_mut().enumerate() {
            *value = (0..4).map(|k| parent[k][row] * local[column][k]).sum();
        }
    }
    result
}

fn engine_position(matrix: Matrix, p: [f32; 3]) -> Vec3 {
    let v = [p[0], p[1], p[2], 1.];
    let component = |row| (0..4).map(|k| matrix[k][row] * v[k]).sum::<f32>();
    // glTF is Y-up; the world is Z-up. Preserve handedness with a quarter turn.
    Vec3::new(component(0), -component(2), component(1))
}

fn normal_matrix(matrix: Matrix) -> Option<Matrix> {
    let [a, b, c] = [0, 1, 2].map(|i| Vec3::new(matrix[i][0], matrix[i][1], matrix[i][2]));
    let cross = |a: Vec3, b: Vec3| {
        Vec3::new(
            a.y * b.z - a.z * b.y,
            a.z * b.x - a.x * b.z,
            a.x * b.y - a.y * b.x,
        )
    };
    let columns = [cross(b, c), cross(c, a), cross(a, b)];
    let determinant = a.dot(columns[0]);
    if !determinant.is_finite() || determinant == 0. {
        return None;
    }
    // Inverse transpose keeps normals perpendicular under nonuniform scaling.
    let mut result = IDENTITY;
    for (i, column) in columns.iter().enumerate() {
        let n = column.scaled(1. / determinant);
        result[i] = [n.x, n.y, n.z, 0.];
    }
    Some(result)
}

fn unit_normal(n: Vec3) -> Option<Vec3> {
    let length = n.dot(n).sqrt();
    (n.finite() && length.is_finite() && length > 0.).then(|| n.scaled(1. / length))
}

fn append_node(
    node: gltf::Node<'_>,
    parent: Matrix,
    selected: &[&str],
    inherited: bool,
    buffers: &[gltf::buffer::Data],
    output: &mut Mesh,
) -> Option<()> {
    let matrix = compose(parent, node.transform().matrix());
    let include = inherited
        || selected.is_empty()
        || node.name().is_some_and(|name| selected.contains(&name));
    if let Some(mesh) = node.mesh().filter(|_| include) {
        for primitive in mesh.primitives() {
            if primitive.mode() != gltf::mesh::Mode::Triangles {
                continue;
            }
            let reader =
                primitive.reader(|buffer| buffers.get(buffer.index()).map(|data| data.as_ref()));
            let positions: Vec<Vec3> = reader
                .read_positions()?
                .map(|p| engine_position(matrix, p))
                .collect();
            let count = u32::try_from(positions.len()).ok()?;
            let normals = if let Some(normals) = reader.read_normals() {
                let transform = normal_matrix(matrix)?;
                let normals = normals
                    .map(|n| unit_normal(engine_position(transform, n)))
                    .collect::<Option<Vec<_>>>()?;
                if normals.len() != positions.len() {
                    return None;
                }
                normals
            } else {
                // Zero selects geometric face lighting for primitives without normals.
                vec![Vec3::default(); positions.len()]
            };
            let local_indices: Vec<u32> = reader
                .read_indices()
                .map(|values| values.into_u32().collect())
                .unwrap_or_else(|| (0..count).collect());
            if local_indices.len() % 3 != 0
                || local_indices.iter().any(|&i| i >= count)
                || positions
                    .iter()
                    .any(|p| !p.x.is_finite() || !p.y.is_finite() || !p.z.is_finite())
            {
                return None;
            }
            let offset = u32::try_from(output.vertices.len()).ok()?;
            offset.checked_add(count)?;
            output.vertices.extend(positions);
            output.normals.extend(normals);
            output
                .indices
                .extend(local_indices.into_iter().map(|i| offset + i));
        }
    }
    for child in node.children() {
        append_node(child, matrix, selected, include, buffers, output)?;
    }
    Some(())
}

/// Loads named node subtrees from the default (or first) scene; empty selection loads all.
/// Applies node transforms, converts to Z-up, and uniformly fits geometry into a unit box.
/// Returns `None` for import failures, invalid geometry, or an empty selection result.
pub fn load_gltf_mesh(path: &Path, selected: &[&str]) -> Option<Mesh> {
    let (document, buffers, _) = gltf::import(path).ok()?;
    let mut output = Mesh::default();

    let scene = document
        .default_scene()
        .or_else(|| document.scenes().next())?;
    for node in scene.nodes() {
        append_node(node, IDENTITY, selected, false, &buffers, &mut output)?;
    }

    let vertices = &mut output.vertices;
    if vertices.is_empty() || output.indices.is_empty() || output.indices.len() % 3 != 0 {
        return None;
    }
    let mut min = vertices[0];
    let mut max = vertices[0];
    for vertex in &vertices[1..] {
        min.x = min.x.min(vertex.x);
        min.y = min.y.min(vertex.y);
        min.z = min.z.min(vertex.z);
        max.x = max.x.max(vertex.x);
        max.y = max.y.max(vertex.y);
        max.z = max.z.max(vertex.z);
    }
    let extent = max - min;
    let scale = extent.x.max(extent.y).max(extent.z).max(f32::EPSILON);
    for vertex in vertices {
        *vertex = (*vertex - min).scaled(1.0 / scale);
    }
    Some(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parent_transform_is_applied_before_conversion_to_z_up() {
        let mut parent = IDENTITY;
        parent[0][0] = 2.;
        parent[3] = [10., 20., 30., 1.];
        let mut child = IDENTITY;
        child[3] = [3., 4., 5., 1.];
        assert_eq!(
            engine_position(compose(parent, child), [1., 2., 3.]),
            Vec3::new(18., -38., 26.)
        );
    }

    #[test]
    fn normals_remain_perpendicular_after_nonuniform_and_mirrored_scale() {
        for x_scale in [2., -2.] {
            let mut matrix = IDENTITY;
            matrix[0][0] = x_scale;
            matrix[1][1] = 3.;
            matrix[2][2] = 4.;
            matrix[3] = [10., 20., 30., 1.];
            let n = unit_normal(engine_position(
                normal_matrix(matrix).unwrap(),
                [-1., 0., 1.],
            ))
            .unwrap();
            let tangent =
                engine_position(matrix, [1., 0., 1.]) - engine_position(matrix, [0., 0., 0.]);
            assert!(n.dot(tangent).abs() < 0.0001);
            assert!((n.dot(n) - 1.).abs() < 0.0001);
            assert_eq!(n.x.is_sign_negative(), x_scale > 0.);
        }
        let mut singular = IDENTITY;
        singular[0][0] = 0.;
        assert!(normal_matrix(singular).is_none());
    }

    #[test]
    fn primitive_without_normals_uses_face_lighting_sentinel() {
        let gltf = gltf::Gltf::from_slice(
            br#"{
            "asset":{"version":"2.0"}, "scene":0,
            "scenes":[{"nodes":[0]}], "nodes":[{"mesh":0}],
            "meshes":[{"primitives":[{"attributes":{"POSITION":0}}]}],
            "buffers":[{"byteLength":36}],
            "bufferViews":[{"buffer":0,"byteLength":36}],
            "accessors":[{"bufferView":0,"componentType":5126,"count":3,
                "type":"VEC3","min":[0,0,0],"max":[1,1,0]}]
        }"#,
        )
        .unwrap();
        let bytes = [0_f32, 0., 0., 1., 0., 0., 0., 1., 0.]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect();
        let mut mesh = Mesh::default();
        append_node(
            gltf.nodes().next().unwrap(),
            IDENTITY,
            &[],
            false,
            &[gltf::buffer::Data(bytes)],
            &mut mesh,
        )
        .unwrap();
        assert_eq!(mesh.indices, [0, 1, 2]);
        assert_eq!(mesh.normals, vec![Vec3::default(); 3]);
    }
}
