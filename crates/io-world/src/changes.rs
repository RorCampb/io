//! Bounded mutation history. Missing history explicitly requires resynchronization.
use io_types::Bounds;
use std::collections::VecDeque;

const CAPACITY: usize = 4096;

#[derive(Clone, Copy, Debug)]
pub enum ChangeSource {
    Item(u64),
    Terrain,
}

#[derive(Clone, Copy, Debug)]
pub struct WorldChange {
    pub sequence: u64,
    pub source: ChangeSource,
    pub before: Option<Bounds>,
    pub after: Option<Bounds>,
    /// Potential sight obstruction change, not evidence that anyone saw it.
    pub occlusion: bool,
}

#[derive(Clone, Debug)]
pub struct ChangeLog {
    identity: u64,
    sequence: u64,
    records: VecDeque<WorldChange>,
}
impl Default for ChangeLog {
    fn default() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let identity = NEXT
            .fetch_update(
                std::sync::atomic::Ordering::Relaxed,
                std::sync::atomic::Ordering::Relaxed,
                |v| v.checked_add(1),
            )
            .expect("world identity exhausted");
        Self {
            identity,
            sequence: 0,
            records: VecDeque::new(),
        }
    }
}
impl ChangeLog {
    /// Stable across snapshots, distinct for independently constructed worlds.
    pub fn identity(&self) -> u64 {
        self.identity
    }
    pub fn cursor(&self) -> u64 {
        self.sequence
    }
    /// None means unavailable history (or a cursor from a newer publication).
    /// Consumers must resample, never treat that as "nothing changed".
    pub fn since(&self, cursor: u64) -> Option<impl Iterator<Item = &WorldChange>> {
        if cursor > self.sequence
            || self
                .records
                .front()
                .is_some_and(|first| cursor < first.sequence - 1)
        {
            return None;
        }
        let skip = self.records.len() - (self.sequence - cursor) as usize;
        Some(self.records.range(skip..))
    }
    pub(crate) fn record(
        &mut self,
        source: ChangeSource,
        before: Option<Bounds>,
        after: Option<Bounds>,
        occlusion: bool,
    ) {
        self.sequence = self
            .sequence
            .checked_add(1)
            .expect("world change sequence exhausted");
        if self.records.len() == CAPACITY {
            self.records.pop_front();
        }
        self.records.push_back(WorldChange {
            sequence: self.sequence,
            source,
            before,
            after,
            occlusion,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn overflow_and_future_cursors_require_resampling() {
        let mut log = ChangeLog::default();
        assert_eq!(log.since(0).unwrap().count(), 0);
        for _ in 0..=CAPACITY {
            log.record(ChangeSource::Terrain, None, None, true);
        }
        assert!(log.since(0).is_none());
        assert_eq!(log.since(1).unwrap().count(), CAPACITY);
        assert!(log.since(log.cursor() + 1).is_none());
    }
    #[test]
    fn rejected_and_noop_poses_do_not_publish_and_snapshots_keep_their_history() {
        use crate::{Item, Space, World};
        use io_types::Vec3;
        let mut world = World::new(
            Space::new(Vec3::new(20., 20., 20.)),
            vec![Item {
                id: 7,
                ..Default::default()
            }],
        );
        let snapshot = world.snapshot();
        let cursor = world.changes().cursor();
        assert!(world.set_pose(7, Vec3::default(), 0.));
        assert!(!world.set_pose(7, Vec3::new(f32::NAN, 0., 0.), 0.));
        assert_eq!(world.changes().cursor(), cursor);
        assert!(world.set_pose(7, Vec3::new(4., 0., 0.), 0.));
        let change = world.changes().since(cursor).unwrap().next().unwrap();
        assert!(matches!(change.source, ChangeSource::Item(7)));
        assert_ne!(change.before, change.after);
        assert_eq!(
            crate::WorldView::changes(&snapshot).unwrap().cursor(),
            cursor
        );
    }
}
