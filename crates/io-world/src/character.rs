//! Conservative upright character sweeps. Physics colliders remain the authority
//! for walls, floors and ceilings; game plugins own velocity and movement rules.
use crate::geometry::{sweep_box, OrientedBox, SegmentHit};
use crate::{CharacterBody, ColliderShape, WorldView};
use io_types::{Rotation, Vec3};

fn body_box(p: Vec3, shape: CharacterBody) -> OrientedBox {
    OrientedBox::new(
        p + Vec3::new(0., 0., shape.height * 0.5),
        Rotation::default(),
        Vec3::new(shape.radius, shape.radius, shape.height * 0.5),
    )
}
fn obstacle(item: &crate::Item) -> Option<OrientedBox> {
    if let Some(shape) = item.character_body {
        return Some(body_box(item.transform.anchor, shape));
    }
    let c = item.collider.as_ref()?;
    let t = item.transform;
    let (h, rotation) = match c.shape {
        ColliderShape::Box { half_extents } => (half_extents, t.rotation),
        ColliderShape::Sphere { radius } => {
            (Vec3::new(radius, radius, radius), Rotation::default())
        }
    };
    Some(OrientedBox::new(
        t.anchor + t.rotation.rotate(c.offset),
        rotation,
        h,
    ))
}
pub(crate) fn item_sweep(
    item: &crate::Item,
    start: Vec3,
    end: Vec3,
    shape: CharacterBody,
) -> Option<SegmentHit> {
    sweep_box(&body_box(start, shape), end - start, &obstacle(item)?, 1e-5)
}

/// Continuous upright-box clearance against oriented boxes and other characters.
/// Spheres retain a conservative box envelope. Does not test terrain or support.
pub fn character_colliders_clear(
    world: &dyn WorldView,
    excluded: Option<u64>,
    start: Vec3,
    end: Vec3,
    shape: CharacterBody,
) -> bool {
    let delta = end - start;
    let length = delta.dot(delta).sqrt();
    if !start.finite()
        || !end.finite()
        || !length.is_finite()
        || length > 128.
        || shape.validate().is_err()
    {
        return false;
    }
    let half = Vec3::new(shape.radius, shape.radius, shape.height * 0.5);
    let center = start + delta.scaled(0.5) + Vec3::new(0., 0., half.z);
    !world
        .query(center, length * 0.5 + half.dot(half).sqrt() + 0.01)
        .into_iter()
        .any(|i| {
            let item = &world.items()[i];
            if Some(item.id) == excluded {
                return false;
            }
            item_sweep(item, start, end, shape).is_some()
        })
}

/// Check transient character bodies along an already computed polyline. Scenery,
/// terrain and support are deliberately not recomputed. Includes initial overlap.
pub fn character_path_clear_of_actors(
    world: &dyn WorldView,
    excluded: Option<u64>,
    points: &[Vec3],
    shape: CharacterBody,
) -> bool {
    let Some(&first) = points.first() else {
        return false;
    };
    if shape.validate().is_err() || points.iter().any(|p| !p.finite()) {
        return false;
    }
    let (mut min, mut max) = (first, first);
    for &p in points {
        min = Vec3::new(min.x.min(p.x), min.y.min(p.y), min.z.min(p.z));
        max = Vec3::new(max.x.max(p.x), max.y.max(p.y), max.z.max(p.z));
    }
    if points.windows(2).any(|p| {
        let d = p[1] - p[0];
        !d.dot(d).is_finite() || d.dot(d) > 128. * 128.
    }) {
        return false;
    }
    min = min - Vec3::new(shape.radius, shape.radius, 0.);
    max = max + Vec3::new(shape.radius, shape.radius, shape.height);
    let center = (min + max).scaled(0.5);
    let half = (max - min).scaled(0.5);
    let radius = half.dot(half).sqrt() + 0.01;
    if !center.finite() || !radius.is_finite() {
        return false;
    }
    !world.query(center, radius).into_iter().any(|i| {
        let item = &world.items()[i];
        item.character_body.is_some()
            && Some(item.id) != excluded
            && (item_sweep(item, first, first, shape).is_some()
                || points
                    .windows(2)
                    .any(|p| item_sweep(item, p[0], p[1], shape).is_some()))
    })
}
pub fn character_fits(world: &dyn WorldView, id: u64, p: Vec3, shape: CharacterBody) -> bool {
    fits(world, Some(id), p, shape)
}
/// Clearance for a hypothetical character; excludes no world item ID.
pub fn character_space_fits(world: &dyn WorldView, p: Vec3, shape: CharacterBody) -> bool {
    fits(world, None, p, shape)
}
/// Complete translated-body clearance: colliders plus terrain triangles. Support
/// is a separate question, so this query also applies to jumps and falls.
pub fn character_segment_clear(
    world: &dyn WorldView,
    excluded: Option<u64>,
    start: Vec3,
    end: Vec3,
    shape: CharacterBody,
) -> bool {
    character_colliders_clear(world, excluded, start, end, shape)
        && terrain_fits(world, start, shape)
        && terrain_fits(world, end, shape)
        && world
            .terrain()
            .is_none_or(|t| t.body_segment_clear(start, end, shape))
}
fn terrain_fits(world: &dyn WorldView, p: Vec3, shape: CharacterBody) -> bool {
    world
        .terrain()
        .and_then(|t| t.footprint(p.x, p.y, shape.radius))
        .is_none_or(|(support, _)| p.z >= support.z - 1e-5)
}
pub(crate) fn fits(
    world: &dyn WorldView,
    excluded: Option<u64>,
    p: Vec3,
    shape: CharacterBody,
) -> bool {
    if !p.finite() || shape.validate().is_err() {
        return false;
    }
    if !terrain_fits(world, p, shape) {
        return false;
    }
    character_colliders_clear(world, excluded, p, p, shape)
}
#[derive(Clone, Copy, Debug)]
pub struct CharacterSweep {
    pub position: Vec3,
    /// Runtime contact with a supporting surface, not presence of a CharacterBody component.
    pub grounded: bool,
    pub hit_ceiling: bool,
}
pub fn sweep_character(
    world: &dyn WorldView,
    id: u64,
    start: Vec3,
    delta: Vec3,
    shape: CharacterBody,
) -> Result<CharacterSweep, String> {
    shape.validate()?;
    if !start.finite() || !delta.finite() || delta.dot(delta) > 128. * 128. {
        return Err("invalid character sweep".into());
    }
    let mut p = start;
    let steps = (delta.dot(delta).sqrt() / (shape.radius.min(shape.height) * 0.25))
        .ceil()
        .max(1.) as usize;
    let step = delta.scaled(1. / steps as f32);
    let mut grounded = false;
    let mut hit_ceiling = false;
    for _ in 0..steps {
        for offset in [
            Vec3::new(step.x, 0., 0.),
            Vec3::new(0., step.y, 0.),
            Vec3::new(0., 0., step.z),
        ] {
            if offset.dot(offset) == 0. {
                continue;
            }
            if character_segment_clear(world, Some(id), p, p + offset, shape) {
                p = p + offset;
                continue;
            }
            // Refine the last safe point rather than stopping a whole substep early.
            let (mut lo, mut hi) = (0., 1.);
            for _ in 0..14 {
                let mid = (lo + hi) * 0.5;
                if character_segment_clear(world, Some(id), p, p + offset.scaled(mid), shape) {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            p = p + offset.scaled(lo);
            hit_ceiling |= offset.z > 0.;
        }
    }
    grounded |= crate::character_support(world, Some(id), p, shape, crate::SupportProbe::CONTACT)?
        .is_some();
    Ok(CharacterSweep {
        position: p,
        grounded,
        hit_ceiling,
    })
}
