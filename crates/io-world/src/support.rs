//! Character support and slope-following shared by planning and execution.
use crate::{
    character::{fits, item_sweep},
    character_segment_clear, surface_candidates, CharacterBody, SurfaceHit, SurfaceQuery,
    SurfaceSource, WorldView,
};
use io_types::Vec3;

const CONTACT: f32 = 0.003;

/// A bounded search around a feet anchor, not permission to teleport to the result.
#[derive(Clone, Copy, Debug)]
pub struct SupportProbe {
    above: f32,
    below: f32,
}
impl SupportProbe {
    pub const CONTACT: Self = Self {
        above: CONTACT,
        below: CONTACT,
    };
    pub fn new(above: f32, below: f32) -> Result<Self, &'static str> {
        if !above.is_finite()
            || !below.is_finite()
            || !(0. ..=4.).contains(&above)
            || !(0. ..=4.).contains(&below)
            || above + below == 0.
        {
            return Err("support probe needs finite nonnegative distances <=4 and a positive span");
        }
        Ok(Self { above, below })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CharacterSupport {
    /// Feet-center placement for the upright body, possibly above the surface's center sample.
    pub anchor: Vec3,
    /// Identifies the physical surface and its normal; not a gameplay permission.
    pub surface: SurfaceHit,
}

fn slope_allows(shape: CharacterBody, normal: Vec3) -> bool {
    normal.z > 0. && normal.x.hypot(normal.y) <= shape.max_slope * normal.z + 1e-5
}

/// Find the highest slope-eligible support in a bounded anchor range with body
/// clearance. Collider support requires a surface beneath the feet center, not
/// just contact with a wall or a character. Terrain clearance covers the square
/// footprint. Boxes retain orientation; sphere obstacles still use a conservative
/// box envelope and are not accepted as supporting surfaces yet.
pub fn character_support(
    world: &dyn WorldView,
    excluded: Option<u64>,
    reference: Vec3,
    shape: CharacterBody,
    probe: SupportProbe,
) -> Result<Option<CharacterSupport>, String> {
    shape.validate()?;
    if !reference.finite() {
        return Err("invalid support reference".into());
    }
    let low = reference.z - probe.below;
    let high = reference.z + probe.above;
    let mut query = SurfaceQuery::new(
        reference.x,
        reference.y,
        low - shape.radius * shape.max_slope * 2. - CONTACT,
        high + CONTACT,
    )
    .map_err(|e| e.to_string())?;
    if let Some(id) = excluded {
        query = query.excluding(id);
    }
    let mut best: Option<CharacterSupport> = None;
    for mut surface in surface_candidates(world, query) {
        if !slope_allows(shape, surface.normal) {
            continue;
        }
        let anchor = match surface.source {
            SurfaceSource::Terrain => {
                let Some((position, normal)) = world
                    .terrain()
                    .and_then(|t| t.footprint(reference.x, reference.y, shape.radius))
                else {
                    continue;
                };
                if !slope_allows(shape, normal) {
                    continue;
                }
                surface.position = position;
                surface.normal = normal;
                Vec3::new(reference.x, reference.y, position.z)
            }
            SurfaceSource::Collider(id) => {
                let Some(item) = world.item(id) else { continue };
                if !matches!(
                    item.collider.map(|c| c.shape),
                    Some(crate::ColliderShape::Box { .. })
                ) {
                    continue;
                }
                if surface.normal == Vec3::new(0., 0., 1.) {
                    surface.position
                } else {
                    // A body sweep raises a flat bottom above the uphill ramp edge.
                    let start = Vec3::new(reference.x, reference.y, high + CONTACT);
                    let end = Vec3::new(reference.x, reference.y, low - CONTACT);
                    let Some(hit) = item_sweep(item, start, end, shape) else {
                        continue;
                    };
                    if hit.normal.is_none_or(|n| n.z <= 0.) {
                        continue;
                    }
                    Vec3::new(
                        reference.x,
                        reference.y,
                        (start.z as f64 + (end.z - start.z) as f64 * hit.fraction) as f32 + 0.00002,
                    )
                }
            }
        };
        if anchor.z < low - 1e-5 || anchor.z > high + 1e-5 || !fits(world, excluded, anchor, shape)
        {
            continue;
        }
        if best.is_none_or(|s| anchor.z > s.anchor.z) {
            best = Some(CharacterSupport { anchor, surface });
        }
    }
    Ok(best)
}

#[derive(Clone, Copy, Debug)]
pub struct CharacterWalk {
    pub position: Vec3,
    pub support: Option<CharacterSupport>,
    /// False means walking stopped before the requested horizontal displacement.
    pub reached: bool,
}

/// Follow continuous slopes without auto-jumping gaps or stepping over vertical
/// obstacles. No mutation, gravity, axis sliding or animation is performed here.
/// Players may fall/slide when blocked; a planner must instead reject that edge.
pub fn walk_character(
    world: &dyn WorldView,
    excluded: Option<u64>,
    start: Vec3,
    delta: Vec3,
    shape: CharacterBody,
) -> Result<CharacterWalk, String> {
    walk_recorded(world, excluded, start, delta, shape, None)
}

/// Same calculation as `walk_character`, retaining every accepted segment,
/// including subdivisions at slope creases. The caller owns reusable storage.
/// This is geometry data, not permission to apply movement against a changed world.
pub fn trace_character_walk(
    world: &dyn WorldView,
    excluded: Option<u64>,
    start: Vec3,
    delta: Vec3,
    shape: CharacterBody,
    points: &mut Vec<Vec3>,
) -> Result<CharacterWalk, String> {
    points.clear();
    walk_recorded(world, excluded, start, delta, shape, Some(points))
}

fn walk_recorded(
    world: &dyn WorldView,
    excluded: Option<u64>,
    start: Vec3,
    delta: Vec3,
    shape: CharacterBody,
    mut trace: Option<&mut Vec<Vec3>>,
) -> Result<CharacterWalk, String> {
    shape.validate()?;
    let length = delta.x.hypot(delta.y);
    if !start.finite() || !delta.finite() || delta.z != 0. || !length.is_finite() || length > 128. {
        return Err("walking needs a finite planar displacement <=128".into());
    }
    if let Some(points) = trace.as_mut() {
        points.push(start);
    }
    let mut result = CharacterWalk {
        position: start,
        support: character_support(world, excluded, start, shape, SupportProbe::CONTACT)?,
        reached: false,
    };
    if result.support.is_none() || !fits(world, excluded, start, shape) {
        if let Some(points) = trace.as_mut() {
            points.clear();
        }
        return Ok(result);
    }
    let steps = (length / (shape.radius * 0.25)).ceil().max(1.) as usize;
    for i in 1..=steps {
        let xy = start + delta.scaled(i as f32 / steps as f32);
        let mut budget = 64;
        let checkpoint = trace.as_ref().map_or(0, |points| points.len());
        let Some(support) = support_step(
            world,
            excluded,
            result.position,
            xy,
            shape,
            &mut budget,
            0,
            &mut trace,
        )?
        else {
            // Failed recursive refinement must not leak points beyond the accepted prefix.
            if let Some(points) = trace.as_mut() {
                points.truncate(checkpoint);
            }
            return Ok(result);
        };
        result.position = support.anchor;
        result.support = Some(support);
    }
    result.reached = true;
    Ok(result)
}

#[allow(clippy::too_many_arguments)]
fn support_step(
    world: &dyn WorldView,
    excluded: Option<u64>,
    start: Vec3,
    target: Vec3,
    shape: CharacterBody,
    budget: &mut u8,
    depth: u8,
    trace: &mut Option<&mut Vec<Vec3>>,
) -> Result<Option<CharacterSupport>, String> {
    if *budget == 0 {
        return Ok(None);
    }
    *budget -= 1;
    let horizontal = (target.x - start.x).hypot(target.y - start.y);
    let rise = shape.max_slope * horizontal + CONTACT;
    let probe = SupportProbe::new(rise, rise).map_err(str::to_owned)?;
    let reference = Vec3::new(target.x, target.y, start.z);
    let Some(support) = character_support(world, excluded, reference, shape, probe)? else {
        return Ok(None);
    };
    if character_segment_clear(world, excluded, start, support.anchor, shape) {
        if let Some(points) = trace.as_mut() {
            points.push(support.anchor);
        }
        return Ok(Some(support));
    }
    if depth >= 16 || horizontal <= 1e-6 {
        return Ok(None);
    }
    // A chord across a slope crease can cut through otherwise walkable support.
    // Refine only failed connectors, checking every resulting segment. Exhaustion
    // stops safely instead of permitting penetration or inventing a jump.
    let middle = (start + reference).scaled(0.5);
    let Some(first) = support_step(
        world,
        excluded,
        start,
        middle,
        shape,
        budget,
        depth + 1,
        trace,
    )?
    else {
        return Ok(None);
    };
    support_step(
        world,
        excluded,
        first.anchor,
        target,
        shape,
        budget,
        depth + 1,
        trace,
    )
}

/// Same supported walking calculation as the motor, with a checked 3D endpoint.
pub fn character_supported_segment(
    world: &dyn WorldView,
    excluded: Option<u64>,
    start: Vec3,
    end: Vec3,
    shape: CharacterBody,
) -> bool {
    if !end.finite() {
        return false;
    }
    walk_character(
        world,
        excluded,
        start,
        Vec3::new(end.x - start.x, end.y - start.y, 0.),
        shape,
    )
    .is_ok_and(|walk| walk.reached && (walk.position.z - end.z).abs() <= CONTACT)
}
