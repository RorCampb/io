use crate::spatial::SpatialIndex;
use crate::{Item, PhysicsStats, Space, World};
use io_types::Vec3;
use std::collections::HashMap;

/// Common read-only surface for synchronous worlds and immutable publications.
pub trait WorldView {
    fn terrain(&self) -> Option<&crate::HeightField> {
        None
    }
    fn space(&self) -> &Space;
    fn items(&self) -> &[Item];
    fn item(&self, id: u64) -> Option<&Item>;
    fn query(&self, center: Vec3, radius: f32) -> Vec<usize>;
    fn revision(&self) -> u64;
    fn spatial_revision(&self) -> u64;
    fn physics_stats(&self) -> PhysicsStats;
    fn physics_error(&self) -> Option<&str>;
}

/// Owned item/spatial data only: no solver caches, meshes, or mutable world access.
pub struct WorldSnapshot {
    pub(crate) terrain: Option<std::sync::Arc<crate::HeightField>>,
    pub(crate) space: Space,
    pub(crate) items: Vec<Item>,
    pub(crate) index: SpatialIndex,
    pub(crate) by_id: HashMap<u64, usize>,
    pub(crate) revision: u64,
    pub(crate) spatial_revision: u64,
    pub(crate) physics_stats: PhysicsStats,
    pub(crate) physics_error: Option<String>,
}

impl WorldView for WorldSnapshot {
    fn terrain(&self) -> Option<&crate::HeightField> {
        self.terrain.as_deref()
    }
    fn space(&self) -> &Space {
        &self.space
    }
    fn items(&self) -> &[Item] {
        &self.items
    }
    fn item(&self, id: u64) -> Option<&Item> {
        self.by_id.get(&id).map(|&i| &self.items[i])
    }
    fn query(&self, center: Vec3, radius: f32) -> Vec<usize> {
        if !center.finite() || !radius.is_finite() || radius < 0. {
            return Vec::new();
        }
        self.index.query(center, radius)
    }
    fn revision(&self) -> u64 {
        self.revision
    }
    fn spatial_revision(&self) -> u64 {
        self.spatial_revision
    }
    fn physics_stats(&self) -> PhysicsStats {
        self.physics_stats
    }
    fn physics_error(&self) -> Option<&str> {
        self.physics_error.as_deref()
    }
}

impl WorldView for World {
    fn terrain(&self) -> Option<&crate::HeightField> {
        self.terrain()
    }
    fn space(&self) -> &Space {
        self.space()
    }
    fn items(&self) -> &[Item] {
        self.items()
    }
    fn item(&self, id: u64) -> Option<&Item> {
        self.item(id)
    }
    fn query(&self, center: Vec3, radius: f32) -> Vec<usize> {
        self.query(center, radius)
    }
    fn revision(&self) -> u64 {
        self.revision()
    }
    fn spatial_revision(&self) -> u64 {
        self.spatial_revision()
    }
    fn physics_stats(&self) -> PhysicsStats {
        self.physics_stats()
    }
    fn physics_error(&self) -> Option<&str> {
        self.physics_error()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn snapshot_retains_item_and_spatial_state_after_world_changes() {
        let mut world = World::new(
            Space::new(Vec3::new(1000., 1000., 1000.)),
            vec![Item {
                id: 1,
                ..Item::default()
            }],
        );
        let snapshot = world.snapshot();
        let old_revision = snapshot.revision();
        assert!(world.set_pose(1, Vec3::new(100., 100., 0.), 0.));
        assert_eq!(snapshot.item(1).unwrap().transform.anchor, Vec3::default());
        assert_eq!(snapshot.revision(), old_revision);
        assert!(snapshot.query(Vec3::new(100., 100., 0.), 1.).is_empty());
        assert_eq!(world.query(Vec3::new(100., 100., 0.), 1.), vec![0]);
        assert!(snapshot.query(Vec3::default(), f32::NAN).is_empty());
    }
}
