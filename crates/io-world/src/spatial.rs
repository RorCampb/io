#![forbid(unsafe_code)]
use io_types::{Bounds, Vec3};
use std::collections::{BTreeMap, HashSet};

const CELL: f32 = 32.;
#[derive(Default)]
pub struct SpatialIndex {
    rows: BTreeMap<i32, BTreeMap<i32, Vec<usize>>>,
    large: Vec<(usize, Bounds)>,
}
impl SpatialIndex {
    pub fn remove(&mut self, id: usize, b: Bounds) {
        let (x0, x1) = (
            (b.min.x / CELL).floor() as i32,
            (b.max.x / CELL).floor() as i32,
        );
        let (y0, y1) = (
            (b.min.y / CELL).floor() as i32,
            (b.max.y / CELL).floor() as i32,
        );
        self.large.retain(|&(item, _)| item != id);
        let mut empty_rows = Vec::new();
        for (&x, row) in self.rows.range_mut(x0..=x1) {
            let mut empty_cells = Vec::new();
            for (&y, items) in row.range_mut(y0..=y1) {
                items.retain(|&item| item != id);
                if items.is_empty() {
                    empty_cells.push(y);
                }
            }
            for y in empty_cells {
                row.remove(&y);
            }
            if row.is_empty() {
                empty_rows.push(x);
            }
        }
        for x in empty_rows {
            self.rows.remove(&x);
        }
    }
    pub fn insert(&mut self, id: usize, b: Bounds) {
        let (x0, x1) = (
            (b.min.x / CELL).floor() as i32,
            (b.max.x / CELL).floor() as i32,
        );
        let (y0, y1) = (
            (b.min.y / CELL).floor() as i32,
            (b.max.y / CELL).floor() as i32,
        );
        let width = i64::from(x1) - i64::from(x0) + 1;
        let height = i64::from(y1) - i64::from(y0) + 1;
        if width > 64 || height > 64 || width * height > 64 {
            self.large.push((id, b));
            return;
        }
        for x in x0..=x1 {
            for y in y0..=y1 {
                self.rows
                    .entry(x)
                    .or_default()
                    .entry(y)
                    .or_default()
                    .push(id);
            }
        }
    }
    pub fn query(&self, center: Vec3, radius: f32) -> Vec<usize> {
        let x0 = ((center.x - radius) / CELL).floor() as i32;
        let x1 = ((center.x + radius) / CELL).floor() as i32;
        let y0 = ((center.y - radius) / CELL).floor() as i32;
        let y1 = ((center.y + radius) / CELL).floor() as i32;
        let mut found = HashSet::new();
        for (_, row) in self.rows.range(x0..=x1) {
            for (_, ids) in row.range(y0..=y1) {
                found.extend(ids.iter().copied());
            }
        }
        for &(id, b) in &self.large {
            if b.within_radius(center, radius) {
                found.insert(id);
            }
        }
        let mut result: Vec<_> = found.into_iter().collect();
        result.sort_unstable();
        result
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn extreme_finite_bounds_use_overflow_storage_without_integer_overflow() {
        let mut index = SpatialIndex::default();
        index.insert(
            7,
            Bounds {
                min: Vec3::new(-1e30, -1e30, 0.),
                max: Vec3::new(1e30, 1e30, 1.),
            },
        );
        assert_eq!(index.query(Vec3::default(), 1.), vec![7]);
        assert!(index.rows.is_empty());
    }
    #[test]
    fn crossing_cells_and_large_items_are_found_once() {
        let mut index = SpatialIndex::default();
        index.insert(
            0,
            Bounds {
                min: Vec3::new(30., 30., 0.),
                max: Vec3::new(35., 35., 2.),
            },
        );
        index.insert(
            1,
            Bounds {
                min: Vec3::new(-1000., -1000., 0.),
                max: Vec3::new(1000., 1000., 1.),
            },
        );
        assert_eq!(index.query(Vec3::new(34., 34., 0.), 1.), vec![0, 1]);
        assert_eq!(index.query(Vec3::new(-500., -500., 0.), 1.), vec![1]);
    }
}
