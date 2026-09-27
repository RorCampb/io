//! Shared segment intersections. Callers validate inputs and choose contact semantics.
use crate::{Collider, ColliderShape, Transform};
use io_types::{Bounds, Rotation, Vec3};

#[derive(Clone, Copy)]
pub(crate) enum Boundary {
    /// Touching blocks sight/casts, including endpoint and tangential contacts.
    Closed,
    /// Character clearance permits touching; only an interval inside blocks travel.
    Open,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct SegmentHit {
    // Keep intersection precision until callers reconstruct a surface point.
    pub fraction: f64,
    /// No unique entry surface when the segment starts inside the solid.
    pub normal: Option<Vec3>,
}

struct Interval {
    enter: f64,
    leave: f64,
    normal: Option<Vec3>,
}
impl Interval {
    fn new() -> Self {
        Self {
            enter: 0.,
            leave: 1.,
            normal: None,
        }
    }
    fn clip(
        &mut self,
        p: f32,
        d: f32,
        lo: f32,
        hi: f32,
        axis: Vec3,
        boundary: Boundary,
    ) -> Option<()> {
        if d == 0. {
            let outside = match boundary {
                Boundary::Closed => p < lo || p > hi,
                Boundary::Open => p <= lo || p >= hi,
            };
            if outside {
                return None;
            }
        } else {
            let (a, b) = (
                (lo as f64 - p as f64) / d as f64,
                (hi as f64 - p as f64) / d as f64,
            );
            if a.min(b) >= self.enter {
                self.enter = a.min(b);
                self.normal = Some(axis.scaled(-d.signum()));
            }
            self.leave = self.leave.min(a.max(b));
        }
        let empty = match boundary {
            Boundary::Closed => self.enter > self.leave,
            Boundary::Open => self.enter >= self.leave,
        };
        (!empty).then_some(())
    }
    fn hit(self) -> SegmentHit {
        SegmentHit {
            fraction: self.enter,
            normal: self.normal,
        }
    }
}

pub(crate) fn segment_box(
    start: Vec3,
    delta: Vec3,
    bounds: Bounds,
    boundary: Boundary,
) -> Option<SegmentHit> {
    let mut interval = Interval::new();
    for (p, d, lo, hi, axis) in [
        (
            start.x,
            delta.x,
            bounds.min.x,
            bounds.max.x,
            Vec3::new(1., 0., 0.),
        ),
        (
            start.y,
            delta.y,
            bounds.min.y,
            bounds.max.y,
            Vec3::new(0., 1., 0.),
        ),
        (
            start.z,
            delta.z,
            bounds.min.z,
            bounds.max.z,
            Vec3::new(0., 0., 1.),
        ),
    ] {
        interval.clip(p, d, lo, hi, axis, boundary)?;
    }
    Some(interval.hit())
}

pub(crate) struct OrientedBox {
    pub center: Vec3,
    pub axes: [Vec3; 3],
    pub half: [f32; 3],
}
impl OrientedBox {
    pub fn new(center: Vec3, rotation: Rotation, half: Vec3) -> Self {
        Self {
            center,
            axes: [
                Vec3::new(1., 0., 0.),
                Vec3::new(0., 1., 0.),
                Vec3::new(0., 0., 1.),
            ]
            .map(|a| rotation.rotate(a)),
            half: [half.x, half.y, half.z],
        }
    }
    pub fn radius(&self, axis: Vec3) -> f32 {
        (0..3)
            .map(|i| self.half[i] * self.axes[i].dot(axis).abs())
            .sum()
    }
}

/// Translating boxes have fixed separating axes. Intersect their overlap time
/// intervals on all face and edge axes; no pose sampling or world-AABB flattening.
pub(crate) fn sweep_box(
    a: &OrientedBox,
    delta: Vec3,
    b: &OrientedBox,
    tolerance: f32,
) -> Option<SegmentHit> {
    let upright = [
        Vec3::new(1., 0., 0.),
        Vec3::new(0., 1., 0.),
        Vec3::new(0., 0., 1.),
    ];
    if a.axes == upright && b.axes == upright {
        let half = Vec3::new(
            a.half[0] + b.half[0] - tolerance,
            a.half[1] + b.half[1] - tolerance,
            a.half[2] + b.half[2] - tolerance,
        );
        return segment_box(
            a.center - b.center,
            delta,
            Bounds {
                min: half.scaled(-1.),
                max: half,
            },
            Boundary::Open,
        );
    }
    let mut interval = Interval::new();
    let axes = a
        .axes
        .into_iter()
        .chain(b.axes)
        .chain(a.axes.into_iter().flat_map(|x| b.axes.map(|y| x.cross(y))));
    for axis in axes {
        let length = axis.dot(axis).sqrt();
        if length < 1e-5 {
            continue;
        }
        let axis = axis.scaled(1. / length);
        let radius = a.radius(axis) + b.radius(axis) - tolerance;
        interval.clip(
            (a.center - b.center).dot(axis),
            delta.dot(axis),
            -radius,
            radius,
            axis,
            Boundary::Open,
        )?;
    }
    Some(interval.hit())
}

/// The same interval solver for a translated box and a stationary triangle.
pub(crate) fn sweep_triangle(
    a: &OrientedBox,
    delta: Vec3,
    triangle: [Vec3; 3],
    tolerance: f32,
) -> Option<SegmentHit> {
    let edges = [
        triangle[1] - triangle[0],
        triangle[2] - triangle[1],
        triangle[0] - triangle[2],
    ];
    let axes = a
        .axes
        .into_iter()
        .chain(std::iter::once(edges[0].cross(edges[1])))
        .chain(
            a.axes
                .into_iter()
                .flat_map(|axis| edges.map(|edge| axis.cross(edge))),
        );
    let mut interval = Interval::new();
    for axis in axes {
        let length = axis.dot(axis).sqrt();
        if length < 1e-5 {
            continue;
        }
        let axis = axis.scaled(1. / length);
        let projections = triangle.map(|p| (p - a.center).dot(axis));
        let min = projections.into_iter().fold(f32::INFINITY, f32::min);
        let max = projections.into_iter().fold(f32::NEG_INFINITY, f32::max);
        let radius = a.radius(axis) - tolerance;
        interval.clip(
            0.,
            delta.dot(axis),
            min - radius,
            max + radius,
            axis,
            Boundary::Open,
        )?;
    }
    Some(interval.hit())
}

fn segment_sphere(start: Vec3, delta: Vec3, radius: f32) -> Option<SegmentHit> {
    let p = [start.x as f64, start.y as f64, start.z as f64];
    let d = [delta.x as f64, delta.y as f64, delta.z as f64];
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let r2 = (radius as f64).powi(2);
    let c = dot(p, p) - r2;
    if c < 0. {
        return Some(SegmentHit {
            fraction: 0.,
            normal: None,
        });
    }
    if c == 0. {
        return Some(SegmentHit {
            fraction: 0.,
            normal: Some(start.scaled(1. / radius)),
        });
    }
    let a = dot(d, d);
    if a == 0. {
        return None;
    }
    // Closest approach avoids subtracting nearly equal quadratic terms when
    // a small sphere is queried from far away. Only the local solve uses f64.
    let closest = -dot(p, d) / a;
    let q = std::array::from_fn(|i| p[i] + d[i] * closest);
    let remaining = r2 - dot(q, q);
    if remaining < 0. {
        return None;
    }
    let fraction = closest - (remaining / a).sqrt();
    if !(0. ..=1.).contains(&fraction) {
        return None;
    }
    let n: [f64; 3] = std::array::from_fn(|i| p[i] + d[i] * fraction);
    let n = Vec3::new(n[0] as f32, n[1] as f32, n[2] as f32);
    Some(SegmentHit {
        fraction,
        normal: Some(n.scaled(1. / n.dot(n).sqrt())),
    })
}

/// Physical collider units ignore visual scale. A positive padding conservatively
/// boxes rounded box corners, matching the existing camera sphere-cast contract.
pub(crate) fn segment_collider(
    transform: &Transform,
    collider: &Collider,
    start: Vec3,
    delta: Vec3,
    padding: f32,
) -> Option<SegmentHit> {
    let p = transform.rotation.inverse_rotate(start - transform.anchor) - collider.offset;
    let d = transform.rotation.inverse_rotate(delta);
    let hit = match collider.shape {
        ColliderShape::Box { half_extents } => {
            let half = half_extents + Vec3::new(padding, padding, padding);
            segment_box(
                p,
                d,
                Bounds {
                    min: half.scaled(-1.),
                    max: half,
                },
                Boundary::Closed,
            )
        }
        ColliderShape::Sphere { radius } => segment_sphere(p, d, radius + padding),
    }?;
    Some(SegmentHit {
        fraction: hit.fraction,
        normal: hit.normal.map(|n| transform.rotation.rotate(n)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn touching_is_a_query_policy_not_another_intersection_algorithm() {
        let bounds = Bounds {
            min: Vec3::default(),
            max: Vec3::new(1., 1., 1.),
        };
        for (start, delta) in [
            (Vec3::new(-1., 0., 0.5), Vec3::new(3., 0., 0.)),
            (Vec3::new(-1., 0.5, 0.5), Vec3::new(1., 0., 0.)),
            (Vec3::new(-1., 1., 0.5), Vec3::new(2., -2., 0.)),
        ] {
            let closed = segment_box(start, delta, bounds, Boundary::Closed);
            assert!(closed.is_some());
            assert!(segment_box(start, delta, bounds, Boundary::Open).is_none());
        }
        let hit = segment_box(
            Vec3::new(-1., 0.5, 0.5),
            Vec3::new(3., 0., 0.),
            bounds,
            Boundary::Open,
        )
        .unwrap();
        assert_eq!(hit.fraction, 1. / 3.);
        assert_eq!(hit.normal, Some(Vec3::new(-1., 0., 0.)));
        let hit = segment_box(
            Vec3::new(0.5, 0.5, 0.5),
            Vec3::default(),
            bounds,
            Boundary::Closed,
        )
        .unwrap();
        assert_eq!(hit.fraction, 0.);
        assert!(hit.normal.is_none());
    }
}
