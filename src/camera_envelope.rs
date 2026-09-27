//! A fixed-yaw vertical slice of an authored axis-aligned interior.
#![forbid(unsafe_code)]
use io_scene::{CameraRig, RigMotion};
use io_types::{Bounds, Vec3};
#[cfg(test)]
#[path = "camera_envelope_tests.rs"]
mod tests;

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub retreat: f32,
    pub rise: f32,
    pub close: f32,
}
#[derive(Clone, Copy, Debug)]
pub struct Pose {
    pub retreat: f32,
    pub rise: f32,
    pub fov: f32,
}
#[derive(Clone, Debug, Default)]
pub struct Envelope {
    pub pose: Option<Pose>,
    pub blend: f32,
    pub zoom: Option<f32>,
}
#[derive(Clone, Debug)]
pub struct Context {
    pub motion: RigMotion,
    pub region: Option<io_world::Interior>,
    pub foot: Option<Vec3>,
    pub approach: f32,
    pub spaces: Vec<Bounds>,
    pub weight: Option<f32>,
    pub anchored: bool,
}
pub fn portal_spaces(
    regions: &[io_world::Interior],
    portals: &[io_world::Portal],
    rigs: &io_scene::CameraRigs,
) -> Vec<Bounds> {
    use io_world::{InteriorId, SpaceLocation};
    let mut result: Vec<_> = regions
        .iter()
        .map(|r| {
            let mut b = r.bounds;
            b.max.z = r.ceiling.unwrap_or(b.max.z);
            b
        })
        .collect();
    for p in portals {
        let (id, outward) = match (p.from, p.to) {
            (SpaceLocation::Exterior, SpaceLocation::Interior(InteriorId(id))) => {
                (id, p.normal.scaled(-1.))
            }
            (SpaceLocation::Interior(InteriorId(id)), SpaceLocation::Exterior) => (id, p.normal),
            _ => continue,
        };
        let Some(r) = regions
            .get(id as usize)
            .and_then(|r| rigs.interiors.get(&r.name))
        else {
            continue;
        };
        let side = p.tangent().scaled(p.width * 0.5);
        let a = p.center - side - Vec3::new(0., 0., p.height * 0.5);
        let b = p.center + side + Vec3::new(0., 0., p.height * 0.5);
        let end = outward.scaled(r.approach);
        result.push(Bounds {
            min: Vec3::new(
                a.x.min(b.x).min(a.x + end.x).min(b.x + end.x),
                a.y.min(b.y).min(a.y + end.y).min(b.y + end.y),
                a.z,
            ),
            max: Vec3::new(
                a.x.max(b.x).max(a.x + end.x).max(b.x + end.x),
                a.y.max(b.y).max(a.y + end.y).max(b.y + end.y),
                b.z,
            ),
        });
    }
    result
}

/// Only exterior entrances get a virtual approach vestibule. An adjoining room
/// supplies its own bounds and ceiling instead of inheriting the larger room's.
pub fn spaces(regions: &[io_world::Interior], rigs: &io_scene::CameraRigs) -> Vec<Bounds> {
    regions
        .iter()
        .map(|r| {
            let mut b = r.bounds;
            if let Some(z) = r.ceiling {
                b.max.z = b.max.z.min(z);
            }
            let center = b.center();
            let e = b.extent();
            let d = r.entry_direction;
            let distance = (if d.x.abs() > 1e-6 {
                e.x / d.x.abs()
            } else {
                f32::INFINITY
            })
            .min(if d.y.abs() > 1e-6 {
                e.y / d.y.abs()
            } else {
                f32::INFINITY
            });
            let entry = center - d.scaled(distance + 0.01);
            let adjoining = regions.iter().any(|other| {
                other.name != r.name
                    && entry.x >= other.bounds.min.x
                    && entry.x <= other.bounds.max.x
                    && entry.y >= other.bounds.min.y
                    && entry.y <= other.bounds.max.y
                    && b.min.z < other.bounds.max.z
                    && b.max.z > other.bounds.min.z
            });
            if !adjoining {
                let approach = rigs
                    .interiors
                    .get(&r.name)
                    .map_or(rigs.exterior.approach, |r| r.approach);
                let d = d.scaled(approach);
                b.min.x -= d.x.max(0.);
                b.max.x -= d.x.min(0.);
                b.min.y -= d.y.max(0.);
                b.max.y -= d.y.min(0.);
            }
            b
        })
        .collect()
}

fn interval(b: Bounds, p: Vec3, d: Vec3) -> Option<(f32, f32)> {
    let (mut lo, mut hi) = (f32::NEG_INFINITY, f32::INFINITY);
    for (p, d, min, max) in [
        (p.x, d.x, b.min.x, b.max.x),
        (p.y, d.y, b.min.y, b.max.y),
        (p.z, d.z, b.min.z, b.max.z),
    ] {
        if d.abs() < 1e-6 {
            if p < min || p > max {
                return None;
            }
        } else {
            let a = (min - p) / d;
            let z = (max - p) / d;
            lo = lo.max(a.min(z));
            hi = hi.min(a.max(z));
        }
    }
    (lo <= hi).then_some((lo, hi))
}
fn union_exit(spaces: &[Bounds], p: Vec3, d: Vec3) -> Option<f32> {
    let mut intervals: Vec<_> = spaces
        .iter()
        .filter_map(|b| interval(*b, p, d))
        .filter(|(_, hi)| *hi >= 0.)
        .collect();
    intervals.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));
    let mut end = None;
    for (lo, hi) in intervals {
        match end {
            None if lo <= 0. => end = Some(hi),
            Some(e) if lo <= e + 1e-5 => end = Some(e.max(hi)),
            _ => break,
        }
    }
    end
}
fn clearance_exit(spaces: &[Bounds], pivot: Vec3, direction: Vec3, margin: f32) -> Option<f32> {
    let mut allowed = f32::INFINITY;
    for x in [-margin, margin] {
        for y in [-margin, margin] {
            for z in [-margin, margin] {
                allowed = allowed.min(union_exit(spaces, pivot + Vec3::new(x, y, z), direction)?);
            }
        }
    }
    Some(allowed)
}
/// Bound the eye and its clearance corners in the connected union, not meshes.
/// The first gap stops the ray: disjoint floors cannot lend each other space.
fn constrain(pose: &mut Pose, spaces: &[Bounds], pivot: Vec3, yaw: f32, margin: f32) {
    let distance = pose.retreat.hypot(pose.rise);
    if distance < 1e-5 {
        return;
    }
    let (s, c) = yaw.sin_cos();
    let direction = Vec3::new(s * pose.retreat, c * pose.retreat, pose.rise).scaled(1. / distance);
    let allowed = clearance_exit(spaces, pivot, direction, margin)
        .unwrap_or(distance)
        .min(distance);
    let scale = (allowed / distance).clamp(0., 1.);
    pose.retreat *= scale;
    pose.rise *= scale;
}
fn smooth(t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    t * t * (3. - 2. * t)
}

/// Intersect the horizontal orbit ray with the inward-offset room planes.
pub fn limits(
    bounds: Bounds,
    pivot: Vec3,
    yaw: f32,
    margin: f32,
    max_retreat: f32,
    max_rise: f32,
    close: f32,
) -> Option<Limits> {
    let min = bounds.min + Vec3::new(margin, margin, margin);
    let max = bounds.max - Vec3::new(margin, margin, margin);
    if min.x >= max.x
        || min.y >= max.y
        || min.z >= max.z
        || pivot.x < bounds.min.x
        || pivot.x > bounds.max.x
        || pivot.y < bounds.min.y
        || pivot.y > bounds.max.y
        || pivot.z < min.z
        || pivot.z > max.z
    {
        return None;
    }
    let (s, c) = yaw.sin_cos();
    let mut retreat = max_retreat;
    for (p, d, lo, hi) in [(pivot.x, s, min.x, max.x), (pivot.y, c, min.y, max.y)] {
        if d > 1e-6 {
            retreat = retreat.min((hi - p) / d);
        } else if d < -1e-6 {
            retreat = retreat.min((lo - p) / d);
        }
    }
    Some(Limits {
        retreat: retreat.max(0.),
        rise: (max.z - pivot.z).min(max_rise).max(0.),
        close: close.min(retreat.max(0.) * 0.45),
    })
}
pub fn path(l: Limits, zoom: f32, rig: &CameraRig, max_fov: f32) -> Pose {
    let t = if rig.max_zoom > rig.min_zoom {
        (rig.max_zoom / zoom).ln() / (rig.max_zoom / rig.min_zoom).ln()
    } else {
        0.
    };
    let low = (l.close * rig.pitch_degrees.to_radians().tan()).min(l.rise * 0.2);
    let shoulder = (l.retreat * rig.pitch_degrees.to_radians().tan()).min(l.rise * 0.35);
    let retreat = l.close + (l.retreat - l.close) * smooth(t / 0.6);
    let rise =
        low + (shoulder - low) * smooth(t / 0.6) + (l.rise - shoulder) * smooth((t - 0.6) / 0.25);
    let fov = rig.fov_degrees
        + (max_fov.max(rig.fov_degrees) - rig.fov_degrees) * smooth((t - 0.85) / 0.15);
    Pose { retreat, rise, fov }
}
pub struct Step<'a> {
    pub motion: RigMotion,
    pub rig: &'a CameraRig,
    pub region: Option<&'a io_world::Interior>,
    pub foot: Option<Vec3>,
    pub approach: f32,
    pub aspect: f32,
    pub near: f32,
    pub seconds: f32,
    pub spaces: &'a [Bounds],
    pub weight: Option<f32>,
    pub anchored: bool,
}
impl Envelope {
    pub fn update(&mut self, s: Step<'_>) {
        let RigMotion::InteriorEnvelope {
            clearance,
            close_distance,
            max_retreat,
            max_rise,
            max_fov_degrees,
        } = s.motion
        else {
            *self = Self::default();
            return;
        };
        let alpha = -(-s.seconds / s.rig.response_seconds).exp_m1();
        let z = self.zoom.get_or_insert(s.rig.zoom);
        *z = ((*z).ln() + (s.rig.zoom.ln() - (*z).ln()) * alpha).exp();
        let goal = s.region.zip(s.foot).and_then(|(region, foot)| {
            let weight = s
                .weight
                .unwrap_or_else(|| region.proximity(foot, s.approach));
            if weight <= 0. {
                return None;
            }
            let mut b = region.bounds;
            if let Some(ceiling) = region.ceiling {
                b.max.z = b.max.z.min(ceiling);
            }
            // The entrance face is a portal, not a solid wall. A short vestibule
            // lets the camera lower before entry; structural meshes remain separate.
            let d = region.entry_direction.scaled(s.approach);
            b.min.x -= d.x.max(0.);
            b.max.x -= d.x.min(0.);
            b.min.y -= d.y.max(0.);
            b.max.y -= d.y.min(0.);
            let lens = max_fov_degrees.max(s.rig.fov_degrees).to_radians() * 0.5;
            let distance = max_retreat.hypot(max_rise);
            let near = s.near.max(distance * distance / (distance + 1024.));
            let margin =
                clearance + near * (1. + lens.tan().powi(2) * (1. + s.aspect * s.aspect)).sqrt();
            let pivot = foot + Vec3::new(0., 0., s.rig.target_height);
            let mut l = limits(
                b,
                pivot,
                s.rig.yaw_degrees.to_radians(),
                margin,
                max_retreat,
                max_rise,
                close_distance,
            )?;
            let (sine, cosine) = s.rig.yaw_degrees.to_radians().sin_cos();
            if let Some(exit) = clearance_exit(s.spaces, pivot, Vec3::new(sine, cosine, 0.), margin)
            {
                l.retreat = exit.min(max_retreat);
                l.close = close_distance.min(l.retreat * 0.45);
            }
            let goal = if s.anchored {
                let angle = s.rig.pitch_degrees.to_radians();
                let radius =
                    (l.retreat / angle.cos().max(0.001)).min(l.rise / angle.sin().max(0.001));
                let t = if s.rig.max_zoom > s.rig.min_zoom {
                    (s.rig.max_zoom / *z).ln() / (s.rig.max_zoom / s.rig.min_zoom).ln()
                } else {
                    0.
                };
                let distance = radius * (0.15 + 0.85 * smooth(t));
                Pose {
                    retreat: distance * angle.cos(),
                    rise: distance * angle.sin(),
                    fov: s.rig.fov_degrees,
                }
            } else {
                path(l, *z, s.rig, max_fov_degrees)
            };
            Some((goal, l, weight, pivot, margin))
        });
        match goal {
            Some((goal, l, weight, pivot, margin)) => {
                let p = self.pose.get_or_insert(goal);
                p.retreat += (goal.retreat - p.retreat) * alpha;
                p.rise += (goal.rise - p.rise) * alpha;
                p.fov += (goal.fov - p.fov) * alpha;
                // Growth eases; insufficient space cannot retain an unsafe old radius.
                p.retreat = p.retreat.min(l.retreat);
                p.rise = p.rise.min(l.rise);
                constrain(p, s.spaces, pivot, s.rig.yaw_degrees.to_radians(), margin);
                self.blend += (weight - self.blend) * alpha;
            }
            None => self.blend *= 1. - alpha,
        }
    }
}
