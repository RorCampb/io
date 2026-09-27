//! Read-only visibility geometry. Detection and reactions belong to plugins.
use crate::{geometry::segment_collider, WorldView};
use io_types::Vec3;

/// Actors do not occlude sight; physical scenery and the shared terrain do.
pub fn line_of_sight(world: &dyn WorldView, source: u64, target: u64) -> Result<bool, String> {
    let eye = |id| -> Result<Vec3, String> {
        let item = world.item(id).ok_or("unknown sight actor")?;
        Ok(match item.character_body {
            Some(m) => item.transform.anchor + Vec3::new(0., 0., m.height * 0.8),
            None => item.bounds().center(),
        })
    };
    let (start, end) = (eye(source)?, eye(target)?);
    sight_segment(world, start, end, [Some(source), Some(target)])
}

/// Visibility through physical scenery and terrain. Actors do not occlude this query.
pub fn sight_segment_clear(world: &dyn WorldView, start: Vec3, end: Vec3) -> Result<bool, String> {
    sight_segment(world, start, end, [None, None])
}
fn sight_segment(
    world: &dyn WorldView,
    start: Vec3,
    end: Vec3,
    excluded: [Option<u64>; 2],
) -> Result<bool, String> {
    if !start.finite() || !end.finite() || !(end - start).finite() {
        return Err("invalid sight segment".into());
    }
    if world.terrain().is_some_and(|t| t.occludes(start, end)) {
        return Ok(false);
    }
    let delta = end - start;
    for i in world.query(
        (start + end).scaled(0.5),
        delta.dot(delta).sqrt() * 0.5 + 0.01,
    ) {
        let item = &world.items()[i];
        if item.character_body.is_some() || excluded.contains(&Some(item.id)) {
            continue;
        }
        let Some(c) = &item.collider else {
            continue;
        };
        if segment_collider(&item.transform, c, start, delta, 0.).is_some() {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Sample arbitrary Item geometry without treating the observer/target as occluders.
pub fn sight_sample_clear(
    world: &dyn WorldView,
    source: u64,
    target: u64,
    start: Vec3,
    end: Vec3,
) -> Result<bool, String> {
    sight_segment(world, start, end, [Some(source), Some(target)])
}
