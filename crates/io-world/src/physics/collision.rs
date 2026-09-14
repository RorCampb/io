use super::{Body, ColliderShape};
use io_types::Vec3;

pub(super) struct Contact {
    pub point: Vec3,
    pub normal: Vec3,
    pub depth: f32,
}
struct BoxShape {
    center: Vec3,
    axes: [Vec3; 3],
    half: [f32; 3],
}
impl BoxShape {
    fn new(body: &Body, h: Vec3) -> Self {
        Self {
            center: body.center,
            axes: [
                body.rotation.rotate(Vec3::new(1., 0., 0.)),
                body.rotation.rotate(Vec3::new(0., 1., 0.)),
                body.rotation.rotate(Vec3::new(0., 0., 1.)),
            ],
            half: [h.x, h.y, h.z],
        }
    }
    fn radius(&self, n: Vec3) -> f32 {
        (0..3)
            .map(|i| self.half[i] * self.axes[i].dot(n).abs())
            .sum()
    }
    fn edge(&self, axis: usize, direction: Vec3) -> (Vec3, Vec3) {
        let mut center = self.center;
        for i in 0..3 {
            if i != axis {
                center =
                    center + self.axes[i].scaled(self.half[i] * sign(self.axes[i].dot(direction)));
            }
        }
        let d = self.axes[axis].scaled(self.half[axis]);
        (center - d, center + d)
    }
}
fn sign(v: f32) -> f32 {
    if v < 0. {
        -1.
    } else {
        1.
    }
}

pub(super) fn contacts(a: &Body, b: &Body) -> Vec<Contact> {
    match (a.collider.shape, b.collider.shape) {
        (ColliderShape::Sphere { radius: ra }, ColliderShape::Sphere { radius: rb }) => {
            let d = b.center - a.center;
            let distance = d.dot(d).sqrt();
            if distance > ra + rb {
                return Vec::new();
            }
            let normal = if distance > 1e-6 {
                d.scaled(1. / distance)
            } else {
                Vec3::new(1., 0., 0.)
            };
            vec![Contact {
                point: a.center + normal.scaled(ra - (ra + rb - distance) * 0.5),
                normal,
                depth: ra + rb - distance,
            }]
        }
        (ColliderShape::Sphere { radius }, ColliderShape::Box { half_extents }) => {
            sphere_box(a.center, radius, &BoxShape::new(b, half_extents))
        }
        (ColliderShape::Box { half_extents }, ColliderShape::Sphere { radius }) => {
            let mut result = sphere_box(b.center, radius, &BoxShape::new(a, half_extents));
            for c in &mut result {
                c.normal = c.normal.scaled(-1.);
            }
            result
        }
        (ColliderShape::Box { half_extents: ha }, ColliderShape::Box { half_extents: hb }) => {
            box_box(&BoxShape::new(a, ha), &BoxShape::new(b, hb))
        }
    }
}
fn sphere_box(center: Vec3, radius: f32, b: &BoxShape) -> Vec<Contact> {
    let d = center - b.center;
    let local = b.axes.map(|axis| d.dot(axis));
    let mut closest = b.center;
    for (i, value) in local.iter().enumerate() {
        closest = closest + b.axes[i].scaled(value.clamp(-b.half[i], b.half[i]));
    }
    let delta = closest - center;
    let distance = delta.dot(delta).sqrt();
    if distance > radius {
        return Vec::new();
    }
    if distance > 1e-6 {
        let normal = delta.scaled(1. / distance);
        return vec![Contact {
            point: closest + normal.scaled((radius - distance) * 0.5),
            normal,
            depth: radius - distance,
        }];
    }
    let axis = (0..3)
        .min_by(|&i, &j| (b.half[i] - local[i].abs()).total_cmp(&(b.half[j] - local[j].abs())))
        .unwrap();
    let outward = b.axes[axis].scaled(sign(local[axis]));
    let depth = b.half[axis] - local[axis].abs();
    vec![Contact {
        point: center + outward.scaled(depth),
        normal: outward.scaled(-1.),
        depth: radius + depth,
    }]
}

#[derive(Clone, Copy)]
enum Axis {
    FaceA(usize),
    FaceB(usize),
    Edge(usize, usize),
}
fn box_box(a: &BoxShape, b: &BoxShape) -> Vec<Contact> {
    let mut depth = f32::INFINITY;
    let mut normal = Vec3::default();
    let mut best = Axis::FaceA(0);
    let mut axes = Vec::with_capacity(15);
    for i in 0..3 {
        axes.push((a.axes[i], Axis::FaceA(i)));
        axes.push((b.axes[i], Axis::FaceB(i)));
    }
    for i in 0..3 {
        for j in 0..3 {
            axes.push((a.axes[i].cross(b.axes[j]), Axis::Edge(i, j)));
        }
    }
    for (n, kind) in axes {
        let length = n.dot(n).sqrt();
        if length < 1e-5 {
            continue;
        }
        let n = n.scaled(1. / length);
        let distance = (b.center - a.center).dot(n);
        let overlap = a.radius(n) + b.radius(n) - distance.abs();
        if overlap < 0. {
            return Vec::new();
        }
        // Prefer face manifolds when face and edge separation are effectively tied.
        if overlap < depth - 1e-5 {
            depth = overlap;
            normal = n.scaled(sign(distance));
            best = kind;
        }
    }
    match best {
        Axis::FaceA(axis) => face_contacts(a, b, axis, normal, normal),
        Axis::FaceB(axis) => face_contacts(b, a, axis, normal.scaled(-1.), normal),
        Axis::Edge(i, j) => {
            let (a0, a1) = a.edge(i, normal);
            let (b0, b1) = b.edge(j, normal.scaled(-1.));
            let (pa, pb) = closest_segments(a0, a1, b0, b1);
            vec![Contact {
                point: (pa + pb).scaled(0.5),
                normal,
                depth,
            }]
        }
    }
}
fn face_contacts(
    reference: &BoxShape,
    incident: &BoxShape,
    axis: usize,
    outward: Vec3,
    normal: Vec3,
) -> Vec<Contact> {
    let incident_axis = (0..3)
        .max_by(|&i, &j| {
            incident.axes[i]
                .dot(outward)
                .abs()
                .total_cmp(&incident.axes[j].dot(outward).abs())
        })
        .unwrap();
    let center = incident.center
        - incident.axes[incident_axis]
            .scaled(incident.half[incident_axis] * sign(incident.axes[incident_axis].dot(outward)));
    let u = (incident_axis + 1) % 3;
    let v = (incident_axis + 2) % 3;
    let x = incident.axes[u].scaled(incident.half[u]);
    let y = incident.axes[v].scaled(incident.half[v]);
    let mut polygon = vec![
        center + x + y,
        center - x + y,
        center - x - y,
        center + x - y,
    ];
    for i in 0..3 {
        if i != axis {
            for s in [-1., 1.] {
                polygon = clip(
                    polygon,
                    reference.center,
                    reference.axes[i].scaled(s),
                    reference.half[i],
                );
            }
        }
    }
    let plane = reference.center + outward.scaled(reference.half[axis]);
    polygon
        .into_iter()
        .filter_map(|p| {
            let depth = (plane - p).dot(outward);
            (depth >= -1e-5).then_some(Contact {
                point: p + outward.scaled(depth * 0.5),
                normal,
                depth: depth.max(0.),
            })
        })
        .collect()
}
fn clip(polygon: Vec<Vec3>, center: Vec3, axis: Vec3, limit: f32) -> Vec<Vec3> {
    let Some(&last) = polygon.last() else {
        return polygon;
    };
    let mut result = Vec::new();
    let mut previous = last;
    let mut dp = (previous - center).dot(axis) - limit;
    for p in polygon {
        let d = (p - center).dot(axis) - limit;
        if (dp <= 0.) != (d <= 0.) {
            result.push(previous + (p - previous).scaled(dp / (dp - d)));
        }
        if d <= 0. {
            result.push(p);
        }
        previous = p;
        dp = d;
    }
    result
}
fn closest_segments(p: Vec3, q: Vec3, r: Vec3, s: Vec3) -> (Vec3, Vec3) {
    let u = q - p;
    let v = s - r;
    let w = p - r;
    let a = u.dot(u);
    let b = u.dot(v);
    let c = v.dot(v);
    let d = u.dot(w);
    let e = v.dot(w);
    let denominator = a * c - b * b;
    let mut t = if denominator > 1e-10 {
        ((b * e - c * d) / denominator).clamp(0., 1.)
    } else {
        0.
    };
    let k = ((b * t + e) / c).clamp(0., 1.);
    t = ((b * k - d) / a).clamp(0., 1.);
    (p + u.scaled(t), r + v.scaled(k))
}
