use crate::spatial::SpatialIndex;
use crate::{Axis, Effect, Item, Space};
use io_types::{Rotation, Vec3, VisualStateId};
use std::collections::HashMap;

pub struct World {
    space: Space,
    items: Vec<Item>,
    index: SpatialIndex,
    revision: u64,
    spatial_revision: u64,
    by_id: HashMap<u64, usize>,
}
impl World {
    pub fn new(space: Space, items: Vec<Item>) -> Self {
        Self::try_new(space, items).expect("invalid initial world")
    }
    pub fn try_new(space: Space, mut items: Vec<Item>) -> Result<Self, String> {
        let mut index = SpatialIndex::default();
        let mut by_id = HashMap::new();
        for item in &mut items {
            item.transform.snap();
            item.validate()
                .map_err(|e| format!("item {}: {e}", item.id))?;
            if item.durability.as_ref().is_some_and(|d| d.current() == 0) {
                item.apply_depletion();
            }
            item.transform.snap();
        }
        for (id, item) in items.iter().enumerate() {
            if by_id.insert(item.id, id).is_some() {
                return Err(format!("duplicate item ID: {}", item.id));
            }
            index.insert(id, item.visibility_bounds());
        }
        Ok(Self {
            space,
            items,
            index,
            revision: 0,
            spatial_revision: 0,
            by_id,
        })
    }
    pub fn space(&self) -> &Space {
        &self.space
    }
    pub fn items(&self) -> &[Item] {
        &self.items
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn spatial_revision(&self) -> u64 {
        self.spatial_revision
    }
    pub fn item(&self, id: u64) -> Option<&Item> {
        self.by_id.get(&id).map(|&index| &self.items[index])
    }
    pub fn damage(&mut self, target: u64, amount: u32) -> bool {
        let Some(&index) = self.by_id.get(&target) else {
            return false;
        };
        let item = &mut self.items[index];
        let old_bounds = item.visibility_bounds();
        let Some(durability) = &mut item.durability else {
            return false;
        };
        if !durability.damage(amount) {
            return false;
        }
        if durability.current() == 0 {
            item.apply_depletion();
            let new_bounds = item.visibility_bounds();
            if new_bounds != old_bounds {
                self.index.remove(index, old_bounds);
                self.index.insert(index, new_bounds);
                self.spatial_revision = self.spatial_revision.wrapping_add(1);
            }
        }
        self.revision = self.revision.wrapping_add(1);
        true
    }
    /// The caller resolves a valid state through the appearance catalog first.
    pub fn set_visual_state(&mut self, target: u64, state: VisualStateId) -> bool {
        let Some(&index) = self.by_id.get(&target) else {
            return false;
        };
        let Some(renderable) = &mut self.items[index].renderable else {
            return false;
        };
        if state.0 >= renderable.visual_state_count || renderable.visual_state == state {
            return false;
        }
        renderable.visual_state = state;
        self.revision = self.revision.wrapping_add(1);
        true
    }
    pub fn set_pose(&mut self, item_id: u64, anchor: Vec3, yaw: f32) -> bool {
        let Some(&id) = self.by_id.get(&item_id) else {
            return false;
        };
        if !anchor.finite() || !yaw.is_finite() || id >= self.items.len() {
            return false;
        }
        let old = self.items[id].visibility_bounds();
        let item = &mut self.items[id];
        let previous = item.transform;
        item.transform.snap();
        item.transform.anchor = anchor;
        item.transform.rotation = Rotation::yaw(yaw).expect("finite yaw checked above");
        if item.validate().is_err() {
            item.transform = previous;
            return false;
        }
        self.index.remove(id, old);
        self.index.insert(id, item.visibility_bounds());
        self.spatial_revision = self.spatial_revision.wrapping_add(1);
        self.revision = self.revision.wrapping_add(1);
        true
    }
    pub fn query(&self, center: Vec3, radius: f32) -> Vec<usize> {
        if !center.finite() || !radius.is_finite() || radius < 0. {
            return Vec::new();
        }
        self.index.query(center, radius)
    }
    pub fn resize_units(&mut self, axis: Axis, delta: i32) {
        if self.space.resize_subdivisions(axis, delta) {
            self.revision = self.revision.wrapping_add(1);
        }
    }
    pub fn simulate(&mut self, active: &[usize], dt: f32) {
        if !dt.is_finite() || dt <= 0. {
            return;
        }
        let mut changed = false;
        let mut queue = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for &id in active {
            if id >= self.items.len() || !seen.insert(id) {
                continue;
            }
            if let Some(motion) = &mut self.items[id].motion {
                let (anchor, yaw) = motion.advance(dt);
                self.set_pose(self.items[id].id, anchor, yaw);
                changed = true;
            }
            if let Some(animation) = &mut self.items[id].animation {
                changed |= animation.advance(dt);
            }
            let item = &self.items[id];
            if let Some(animation) = &item.animation {
                if let Some(events) = animation.events() {
                    events.enqueue(
                        item.id,
                        animation.previous_time(),
                        animation.time(),
                        f64::from(animation.speed()),
                        &mut queue,
                    );
                }
            }
            if self.items[id].needs_simulation() {
                self.items[id].simulated_ticks = self.items[id].simulated_ticks.wrapping_add(1);
                changed = true;
            }
        }
        if changed {
            self.revision = self.revision.wrapping_add(1);
        }
        // Apply only after all item updates, in tick-time order with stable tie breaks.
        queue.sort_by(|a, b| {
            a.offset
                .total_cmp(&b.offset)
                .then(a.source.cmp(&b.source))
                .then(a.event_index.cmp(&b.event_index))
        });
        for pending in queue {
            pending.effect.apply(pending.target, self);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Renderable, Transform};
    #[test]
    fn invalid_items_and_mutations_cannot_enter_the_spatial_index() {
        let space = || Space::new(Vec3::new(100., 100., 100.));
        let item = Item {
            id: 1,
            ..Item::default()
        };
        assert!(World::try_new(space(), vec![item.clone(), item.clone()]).is_err());
        for invalid in [
            Item {
                id: 0,
                ..item.clone()
            },
            Item {
                transform: Transform {
                    size: Vec3::new(-1., 1., 1.),
                    ..Transform::default()
                },
                ..item.clone()
            },
            Item {
                transform: Transform {
                    anchor: Vec3::new(f32::NAN, 0., 0.),
                    ..Transform::default()
                },
                ..item.clone()
            },
            Item {
                renderable: Some(Renderable {
                    visual_state: VisualStateId(1),
                    ..Renderable::default()
                }),
                ..item.clone()
            },
        ] {
            assert!(World::try_new(space(), vec![invalid]).is_err());
        }
        let mut world = World::try_new(space(), vec![item.clone()]).unwrap();
        let revision = world.revision();
        assert!(!world.set_visual_state(1, VisualStateId(99)));
        assert!(!world.set_pose(1, Vec3::new(f32::NAN, 0., 0.), 0.));
        assert!(world.query(Vec3::default(), -1.).is_empty());
        assert_eq!(world.items()[0], item);
        assert_eq!(world.revision(), revision);
    }
    #[test]
    fn moving_an_item_updates_queries_and_removes_stale_entries() {
        let item = Item {
            id: 42,
            transform: Transform::new(Vec3::new(5., 5., 0.), Vec3::new(1., 1., 1.), 0.).unwrap(),
            ..Item::default()
        };
        let mut world = World::new(Space::new(Vec3::new(1000., 1000., 20.)), vec![item]);
        for step in 1..10 {
            let old = world.items()[0].transform.anchor;
            let new = Vec3::new(100. * step as f32, 100., 0.);
            assert!(world.set_pose(42, new, 0.));
            assert!(!world.query(old, 1.).contains(&0));
            assert_eq!(world.query(new, 1.), vec![0]);
        }
        let saved = world.items()[0].clone();
        assert!(!world.set_pose(42, Vec3::new(f32::NAN, 0., 0.), 0.));
        assert!(!world.set_pose(999, Vec3::default(), 0.));
        assert_eq!(world.item(42), Some(&saved));
    }
    #[test]
    fn subdivisions_do_not_move_or_resize_items() {
        let mut w = World::new(
            Space::new(Vec3::new(10000., 10000., 256.)),
            vec![Item {
                id: 1,
                transform: Transform::new(Vec3::new(10., 10., 0.), Vec3::new(2., 3., 4.), 0.)
                    .unwrap(),
                simulated_ticks: 0,
                ..Item::default()
            }],
        );
        let before = w.items.clone();
        w.resize_units(Axis::A, 15);
        w.resize_units(Axis::B, -200);
        assert_eq!(before, w.items);
        assert_eq!(w.space.dimensions(), Vec3::new(10000., 10000., 256.));
        assert_eq!(w.space.subdivisions(), [16, 1, 1]);
    }
    #[test]
    fn rotated_bounds_contain_all_box_vertices() {
        let item = Item {
            id: 1,
            transform: Transform::new(Vec3::new(10., 10., 0.), Vec3::new(2., 3., 4.), 1.2).unwrap(),
            simulated_ticks: 0,
            ..Item::default()
        };
        let m = item.transform();
        let b = item.bounds();
        let corners = [0., 1.].into_iter().flat_map(|x| {
            [0., 1.]
                .into_iter()
                .flat_map(move |y| [0., 1.].into_iter().map(move |z| Vec3::new(x, y, z)))
        });
        for v in corners {
            let p = Vec3::new(
                m[0] * v.x + m[4] * v.y + m[12],
                m[1] * v.x + m[5] * v.y + m[13],
                m[10] * v.z + m[14],
            );
            assert!(
                p.x >= b.min.x - 0.001
                    && p.x <= b.max.x + 0.001
                    && p.y >= b.min.y - 0.001
                    && p.y <= b.max.y + 0.001
            );
        }
    }
}
