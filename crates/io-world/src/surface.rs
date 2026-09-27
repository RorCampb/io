//! Read-only surface candidates, independent of route planning and game semantics.
use crate::{geometry::segment_collider, WorldView};
use io_types::Vec3;

/// Physical source identity in the queried WorldView. Not a walkability tag or
/// a persistent contact ID; re-query after relevant world geometry changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SurfaceSource {
    Terrain,
    Collider(u64),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurfaceHit {
    pub position: Vec3,
    /// Upward outward unit normal of the collision surface.
    pub normal: Vec3,
    pub source: SurfaceSource,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfaceQueryError {
    NonFinite,
    CoordinatesOutOfRange,
    InvalidVerticalRange,
}
impl std::fmt::Display for SurfaceQueryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NonFinite => "surface query coordinates must be finite",
            Self::CoordinatesOutOfRange => "surface query coordinates must be within +/-1000000",
            Self::InvalidVerticalRange => "surface query requires 0 < max_z - min_z <= 1000",
        })
    }
}
impl std::error::Error for SurfaceQueryError {}

/// A validated vertical column, with inclusive height limits and optional exclusion.
/// This is a point query, not a character-volume sweep or a reachability test.
#[derive(Clone, Copy, Debug)]
pub struct SurfaceQuery {
    start: Vec3,
    min_z: f32,
    exclude: Option<u64>,
}
impl SurfaceQuery {
    pub fn new(x: f32, y: f32, min_z: f32, max_z: f32) -> Result<Self, SurfaceQueryError> {
        let values = [x, y, min_z, max_z];
        if values.iter().any(|v| !v.is_finite()) {
            return Err(SurfaceQueryError::NonFinite);
        }
        if values.iter().any(|v| v.abs() > 1_000_000.) {
            return Err(SurfaceQueryError::CoordinatesOutOfRange);
        }
        if max_z <= min_z || max_z - min_z > 1000. {
            return Err(SurfaceQueryError::InvalidVerticalRange);
        }
        Ok(Self {
            start: Vec3::new(x, y, max_z),
            min_z,
            exclude: None,
        })
    }
    pub fn excluding(mut self, item: u64) -> Self {
        self.exclude = Some(item);
        self
    }
}

/// Enumerate upward terrain/collider surfaces in a column, highest first; ties
/// use source identity, independent of insertion/spatial-query order. Layers do
/// not occlude one another: a bridge and the surface below are separate results.
///
/// Only existing collision geometry participates, not render/occupancy bounds,
/// character bodies or interior volumes. Boxes retain orientation and spheres
/// retain curvature. Physical collider sizes ignore visual scale. A solid whose
/// top is above the query is not reported merely because the query starts inside.
///
/// Candidates do NOT prove body clearance, foot support, acceptable slope or a
/// connected route. The caller must test those using its movement capabilities.
///
/// ```
/// use io_world::{surface_candidates, SurfaceQuery, SurfaceSource, WorldView};
/// # fn inspect(world: &dyn WorldView) -> Result<(), io_world::SurfaceQueryError> {
/// // Keep every layer; do not assume the highest one is the actor's floor.
/// let query = SurfaceQuery::new(10., 20., -2., 12.)?.excluding(42);
/// for surface in surface_candidates(world, query) {
///     match surface.source {
///         SurfaceSource::Terrain => { /* terrain sample */ }
///         SurfaceSource::Collider(item_id) => { /* consult plugin-owned item state */ }
///     }
///     // Body clearance and movement capabilities are separate checks.
/// }
/// # Ok(())
/// # }
/// ```
pub fn surface_candidates(world: &dyn WorldView, query: SurfaceQuery) -> Vec<SurfaceHit> {
    let start = query.start;
    let delta = Vec3::new(0., 0., query.min_z - start.z);
    let mut hits = Vec::new();
    if let Some(sample) = world.terrain().and_then(|t| t.sample(start.x, start.y)) {
        if (query.min_z..=start.z).contains(&sample.height) {
            hits.push(SurfaceHit {
                position: Vec3::new(start.x, start.y, sample.height),
                normal: sample.normal,
                source: SurfaceSource::Terrain,
            });
        }
    }
    for index in world.query(start + delta.scaled(0.5), -delta.z * 0.5 + 0.01) {
        let item = &world.items()[index];
        if Some(item.id) == query.exclude {
            continue;
        }
        let Some(collider) = &item.collider else {
            continue;
        };
        let Some(hit) = segment_collider(&item.transform, collider, start, delta, 0.) else {
            continue;
        };
        let Some(normal) = hit.normal.filter(|n| n.z > 0.) else {
            continue;
        };
        hits.push(SurfaceHit {
            position: Vec3::new(
                start.x,
                start.y,
                (start.z as f64 + delta.z as f64 * hit.fraction) as f32,
            ),
            normal,
            source: SurfaceSource::Collider(item.id),
        });
    }
    hits.sort_by(|a, b| {
        b.position
            .z
            .total_cmp(&a.position.z)
            .then(a.source.cmp(&b.source))
    });
    hits
}
