//! Sampled terrain and a conservative upright character controller, independent of game rules.
use crate::{ColliderShape, WorldView};
use io_types::{Bounds, Vec3};

#[derive(Clone, Debug)]
pub struct HeightField {
    origin: [f32; 2],
    spacing: f32,
    width: usize,
    depth: usize,
    heights: Vec<f32>,
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
        Ok(Self {
            origin,
            spacing,
            width,
            depth,
            heights,
        })
    }
    /// Matches the (00,10,11), (00,11,01) triangulation used by terrain exporters.
    pub fn height(&self, x: f32, y: f32) -> Option<f32> {
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
            a + u * (b - a) + v * (c - b)
        } else {
            a + v * (d - a) + u * (c - d)
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
}

/// Geometry query only: game plugins decide sight range and who detects whom.
/// Actors do not occlude sight; physical scenery and the shared terrain do.
pub fn line_of_sight(world: &dyn WorldView, source: u64, target: u64) -> Result<bool, String> {
    let eye = |id| -> Result<Vec3, String> {
        let item = world.item(id).ok_or("unknown sight actor")?;
        Ok(match item.grounded {
            Some(m) => item.transform.anchor + Vec3::new(0., 0., m.height * 0.8),
            None => item.bounds().center(),
        })
    };
    let (start, end) = (eye(source)?, eye(target)?);
    if world.terrain().is_some_and(|t| t.occludes(start, end)) {
        return Ok(false);
    }
    let delta = end - start;
    for i in world.query(
        (start + end).scaled(0.5),
        delta.dot(delta).sqrt() * 0.5 + 0.01,
    ) {
        let item = &world.items()[i];
        if item.id == source || item.id == target || item.grounded.is_some() {
            continue;
        }
        let Some(c) = &item.collider else {
            continue;
        };
        let p = item
            .transform
            .rotation
            .inverse_rotate(start - item.transform.anchor)
            - c.offset;
        let d = item.transform.rotation.inverse_rotate(delta);
        let blocked = match c.shape {
            ColliderShape::Sphere { radius } => {
                let t = if d.dot(d) > 1e-10 {
                    (-p.dot(d) / d.dot(d)).clamp(0., 1.)
                } else {
                    0.
                };
                let q = p + d.scaled(t);
                q.dot(q) <= radius * radius
            }
            ColliderShape::Box { half_extents: h } => {
                let (mut near, mut far) = (0_f32, 1_f32);
                for (p, d, h) in [(p.x, d.x, h.x), (p.y, d.y, h.y), (p.z, d.z, h.z)] {
                    if d.abs() < 1e-8 {
                        if p.abs() > h {
                            far = -1.;
                            break;
                        }
                    } else {
                        let (a, b) = ((-h - p) / d, (h - p) / d);
                        near = near.max(a.min(b));
                        far = far.min(a.max(b));
                    }
                }
                near <= far
            }
        };
        if blocked {
            return Ok(false);
        }
    }
    Ok(true)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grounded {
    pub radius: f32,
    pub height: f32,
    /// Maximum rise/run, not degrees.
    pub max_slope: f32,
}
impl Grounded {
    pub fn validate(self) -> Result<(), String> {
        if ![self.radius, self.height, self.max_slope]
            .iter()
            .all(|v| v.is_finite())
            || !(0.1..=4.).contains(&self.radius)
            || !(0.2..=8.).contains(&self.height)
            || !(0.1..=2.).contains(&self.max_slope)
        {
            return Err("invalid grounded movement component".into());
        }
        Ok(())
    }
}

pub fn ground_destination(world: &dyn WorldView, id: u64, requested: Vec3) -> Result<Vec3, String> {
    let item = world.item(id).ok_or("unknown walker")?;
    let Some(motor) = item.grounded else {
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
                    let bounds = if let Some(m) = other.grounded {
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
