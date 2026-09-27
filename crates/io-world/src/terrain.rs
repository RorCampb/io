//! Height-field terrain and terrain-following movement, not a classification of walkable Items.
use crate::{ColliderShape, WorldView};
use io_types::{Bounds, Vec3};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TerrainSample {
    pub height: f32,
    /// Upward unit normal of the sampled triangle, not a smoothed render normal.
    pub normal: Vec3,
}

#[derive(Clone, Debug)]
pub struct HeightField {
    origin: [f32; 2],
    spacing: f32,
    width: usize,
    depth: usize,
    heights: Vec<f32>,
    max_slope: f32,
}
impl HeightField {
    pub fn new(
        origin: [f32; 2],
        spacing: f32,
        width: usize,
        depth: usize,
        heights: Vec<f32>,
    ) -> Result<Self, String> {
        if width < 2
            || depth < 2
            || width.checked_mul(depth) != Some(heights.len())
            || heights.len() > 1_000_000
            || !spacing.is_finite()
            || !(0.1..=100.).contains(&spacing)
            || origin
                .iter()
                .chain(heights.iter())
                .any(|v| !v.is_finite() || v.abs() > 100_000.)
            || spacing * width.max(depth) as f32 > 100_000.
        {
            return Err("invalid height field".into());
        }
        let mut max_slope = 0_f32;
        for j in 0..depth - 1 {
            for i in 0..width - 1 {
                let (a, b, c, d) = (
                    heights[j * width + i],
                    heights[j * width + i + 1],
                    heights[(j + 1) * width + i + 1],
                    heights[(j + 1) * width + i],
                );
                max_slope = max_slope
                    .max((b - a).hypot(c - b) / spacing)
                    .max((c - d).hypot(d - a) / spacing);
            }
        }
        Ok(Self {
            origin,
            spacing,
            width,
            depth,
            heights,
            max_slope,
        })
    }
    /// Matches the (00,10,11), (00,11,01) triangulation used by terrain exporters.
    pub fn height(&self, x: f32, y: f32) -> Option<f32> {
        self.height_and_gradient(x, y).map(|(height, _)| height)
    }
    /// Height and exact triangle normal. On a cell diagonal, selects (00,10,11),
    /// consistently with `height`. Outside the height field there is no surface.
    pub fn sample(&self, x: f32, y: f32) -> Option<TerrainSample> {
        let (height, [dx, dy]) = self.height_and_gradient(x, y)?;
        let normal = Vec3::new(-dx, -dy, 1.);
        Some(TerrainSample {
            height,
            normal: normal.scaled(1. / normal.dot(normal).sqrt()),
        })
    }
    /// Maximum over a square footprint. A piecewise-linear triangle can only
    /// peak at a clipped vertex: rectangle corners or its cell diagonal crossings.
    pub(crate) fn footprint(&self, x: f32, y: f32, radius: f32) -> Option<(Vec3, Vec3)> {
        let lo = [
            (x - radius).max(self.origin[0]),
            (y - radius).max(self.origin[1]),
        ];
        let hi = [
            (x + radius).min(self.origin[0] + self.spacing * (self.width - 1) as f32),
            (y + radius).min(self.origin[1] + self.spacing * (self.depth - 1) as f32),
        ];
        if lo[0] > hi[0] || lo[1] > hi[1] {
            return None;
        }
        let cells = |axis: usize, count: usize| {
            let first = ((lo[axis] - self.origin[axis]) / self.spacing).floor() as usize;
            let last = ((hi[axis] - self.origin[axis]) / self.spacing).floor() as usize;
            first.min(count - 2)..=last.min(count - 2)
        };
        let mut best: Option<(Vec3, Vec3)> = None;
        for i in cells(0, self.width) {
            for j in cells(1, self.depth) {
                let x0 = self.origin[0] + i as f32 * self.spacing;
                let y0 = self.origin[1] + j as f32 * self.spacing;
                let (a, b) = (lo[0].max(x0), hi[0].min(x0 + self.spacing));
                let (c, d) = (lo[1].max(y0), hi[1].min(y0 + self.spacing));
                for (px, py) in [
                    (a, c),
                    (a, d),
                    (b, c),
                    (b, d),
                    (a, y0 + a - x0),
                    (b, y0 + b - x0),
                    (x0 + c - y0, c),
                    (x0 + d - y0, d),
                ] {
                    if px < a || px > b || py < c || py > d {
                        continue;
                    }
                    if let Some(sample) = self.sample(px, py) {
                        if best.is_none_or(|(p, _)| sample.height > p.z) {
                            best = Some((Vec3::new(px, py, sample.height), sample.normal));
                        }
                    }
                }
            }
        }
        best
    }
    /// Continuous body/triangle clearance. The caller separately rejects starting
    /// or ending below terrain; missing terrain coverage is not an invisible wall.
    pub(crate) fn body_segment_clear(
        &self,
        start: Vec3,
        end: Vec3,
        shape: crate::CharacterBody,
    ) -> bool {
        let lo = [
            (start.x.min(end.x) - shape.radius).max(self.origin[0]),
            (start.y.min(end.y) - shape.radius).max(self.origin[1]),
        ];
        let hi = [
            (start.x.max(end.x) + shape.radius)
                .min(self.origin[0] + self.spacing * (self.width - 1) as f32),
            (start.y.max(end.y) + shape.radius)
                .min(self.origin[1] + self.spacing * (self.depth - 1) as f32),
        ];
        if lo[0] > hi[0] || lo[1] > hi[1] {
            return true;
        }
        let cells = |axis: usize, count: usize| {
            let first = ((lo[axis] - self.origin[axis]) / self.spacing).floor() as usize;
            let last = ((hi[axis] - self.origin[axis]) / self.spacing).floor() as usize;
            first.min(count - 2)..=last.min(count - 2)
        };
        let body = crate::geometry::OrientedBox::new(
            start + Vec3::new(0., 0., shape.height * 0.5),
            io_types::Rotation::default(),
            Vec3::new(shape.radius, shape.radius, shape.height * 0.5),
        );
        for i in cells(0, self.width) {
            for j in cells(1, self.depth) {
                let point = |x: usize, y: usize| {
                    Vec3::new(
                        self.origin[0] + x as f32 * self.spacing,
                        self.origin[1] + y as f32 * self.spacing,
                        self.heights[y * self.width + x],
                    )
                };
                let (a, b, c, d) = (
                    point(i, j),
                    point(i + 1, j),
                    point(i + 1, j + 1),
                    point(i, j + 1),
                );
                if [[a, b, c], [a, c, d]].into_iter().any(|triangle| {
                    crate::geometry::sweep_triangle(&body, end - start, triangle, 1e-5).is_some()
                }) {
                    return false;
                }
            }
        }
        true
    }
    fn height_and_gradient(&self, x: f32, y: f32) -> Option<(f32, [f32; 2])> {
        let u = (x - self.origin[0]) / self.spacing;
        let v = (y - self.origin[1]) / self.spacing;
        if !u.is_finite()
            || !v.is_finite()
            || u < 0.
            || v < 0.
            || u > (self.width - 1) as f32
            || v > (self.depth - 1) as f32
        {
            return None;
        }
        let i = (u.floor() as usize).min(self.width - 2);
        let j = (v.floor() as usize).min(self.depth - 2);
        let (u, v) = (u - i as f32, v - j as f32);
        let (a, b, c, d) = (
            self.heights[j * self.width + i],
            self.heights[j * self.width + i + 1],
            self.heights[(j + 1) * self.width + i + 1],
            self.heights[(j + 1) * self.width + i],
        );
        Some(if u >= v {
            (
                a + u * (b - a) + v * (c - b),
                [(b - a) / self.spacing, (c - b) / self.spacing],
            )
        } else {
            (
                a + v * (d - a) + u * (c - d),
                [(c - d) / self.spacing, (d - a) / self.spacing],
            )
        })
    }
    /// A segment and each terrain triangle are linear. Test every grid/diagonal
    /// crossing, rather than sampling at a resolution that could miss a ridge.
    pub fn occludes(&self, start: Vec3, end: Vec3) -> bool {
        let below = |t: f32| {
            let p = start + (end - start).scaled(t);
            self.height(p.x, p.y).is_none_or(|h| p.z <= h + 0.001)
        };
        if !start.finite() || !end.finite() || below(0.) || below(1.) {
            return true;
        }
        let u = (start.x - self.origin[0]) / self.spacing;
        let v = (start.y - self.origin[1]) / self.spacing;
        let du = (end.x - start.x) / self.spacing;
        let dv = (end.y - start.y) / self.spacing;
        for (a, d) in [(u, du), (v, dv), (u - v, du - dv)] {
            if d.abs() < 1e-7 {
                continue;
            }
            for k in a.min(a + d).ceil() as i32..=a.max(a + d).floor() as i32 {
                let t = (k as f32 - a) / d;
                if (0. ..=1.).contains(&t) && below(t) {
                    return true;
                }
            }
        }
        false
    }
    /// Conservative sphere clearance using the terrain's maximum triangle slope.
    /// Binary search tests whole prefixes, so a narrow intervening ridge is not skipped.
    pub(crate) fn sphere_fraction(&self, start: Vec3, end: Vec3, radius: f32) -> f32 {
        let offset = Vec3::new(0., 0., radius * (1. + self.max_slope.powi(2)).sqrt());
        let start = start - offset;
        let end = end - offset;
        if !self.occludes(start, end) {
            return 1.;
        }
        if self.occludes(start, start) {
            return 0.;
        }
        let (mut lo, mut hi) = (0., 1.);
        for _ in 0..18 {
            let mid = (lo + hi) * 0.5;
            if self.occludes(start, start + (end - start).scaled(mid)) {
                hi = mid;
            } else {
                lo = mid;
            }
        }
        lo
    }
}

/// Follow the optional height field, or retain current height when it is absent.
/// This is not yet a general query for stacked collider-supported surfaces.
pub fn terrain_destination(
    world: &dyn WorldView,
    id: u64,
    requested: Vec3,
) -> Result<Vec3, String> {
    let item = world.item(id).ok_or("unknown walker")?;
    let Some(motor) = item.character_body else {
        return Ok(requested);
    };
    let start = item.transform.anchor;
    let delta = Vec3::new(requested.x - start.x, requested.y - start.y, 0.);
    let distance = delta.dot(delta).sqrt();
    if !requested.finite() || !distance.is_finite() || distance > 128. {
        return Err("invalid movement distance".into());
    }
    let steps = (distance / (motor.radius * 0.4)).ceil().max(1.) as usize;
    let step = delta.scaled(1. / steps as f32);
    let mut p = start;
    // Small bounded steps prevent tunnelling; try the full direction, then slide on either axis.
    for _ in 0..steps {
        for offset in [step, Vec3::new(step.x, 0., 0.), Vec3::new(0., step.y, 0.)] {
            let mut q = p + offset;
            q.z = match world.terrain() {
                Some(t) => match t.height(q.x, q.y) {
                    Some(h) => h,
                    None => continue,
                },
                None => p.z,
            };
            let horizontal = offset.dot(offset).sqrt();
            if (q.z - p.z).abs() > motor.max_slope * horizontal + 0.03 {
                continue;
            }
            let probe = Bounds {
                min: Vec3::new(q.x - motor.radius, q.y - motor.radius, q.z + 0.08),
                max: Vec3::new(q.x + motor.radius, q.y + motor.radius, q.z + motor.height),
            };
            let blocked = world
                .query(q, motor.height + motor.radius + 2.)
                .iter()
                .any(|&i| {
                    let other = &world.items()[i];
                    if other.id == id {
                        return false;
                    }
                    let bounds = if let Some(m) = other.character_body {
                        let a = other.transform.anchor;
                        Bounds {
                            min: Vec3::new(a.x - m.radius, a.y - m.radius, a.z + 0.08),
                            max: Vec3::new(a.x + m.radius, a.y + m.radius, a.z + m.height),
                        }
                    } else if let Some(c) = &other.collider {
                        let half = match c.shape {
                            ColliderShape::Box { half_extents } => half_extents,
                            ColliderShape::Sphere { radius } => Vec3::new(radius, radius, radius),
                        };
                        let mut transform = other.transform;
                        transform.size = Vec3::new(1., 1., 1.);
                        transform.bounds(Bounds {
                            min: c.offset - half,
                            max: c.offset + half,
                        })
                    } else {
                        return false;
                    };
                    probe.min.x < bounds.max.x
                        && probe.max.x > bounds.min.x
                        && probe.min.y < bounds.max.y
                        && probe.max.y > bounds.min.y
                        && probe.min.z < bounds.max.z
                        && probe.max.z > bounds.min.z
                });
            if !blocked {
                p = q;
                break;
            }
        }
    }
    Ok(p)
}
