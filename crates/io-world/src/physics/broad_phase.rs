use io_types::Bounds;
use std::collections::{BTreeSet, HashMap};

type Cell = (i32, i32, i32);

#[derive(Default)]
pub(super) struct Grid {
    size: f32,
    bounds: Vec<Option<Bounds>>,
    cells: HashMap<Cell, Vec<usize>>,
    large: BTreeSet<usize>,
}

fn range(bounds: Bounds, size: f32) -> Option<(Cell, Cell)> {
    let lo = (
        (bounds.min.x / size).floor() as i32,
        (bounds.min.y / size).floor() as i32,
        (bounds.min.z / size).floor() as i32,
    );
    let hi = (
        (bounds.max.x / size).floor() as i32,
        (bounds.max.y / size).floor() as i32,
        (bounds.max.z / size).floor() as i32,
    );
    let count = (i64::from(hi.0) - i64::from(lo.0) + 1)
        .checked_mul(i64::from(hi.1) - i64::from(lo.1) + 1)?
        .checked_mul(i64::from(hi.2) - i64::from(lo.2) + 1)?;
    (count <= 512).then_some((lo, hi))
}

fn cells((lo, hi): (Cell, Cell)) -> impl Iterator<Item = Cell> {
    (lo.0..=hi.0)
        .flat_map(move |x| (lo.1..=hi.1).flat_map(move |y| (lo.2..=hi.2).map(move |z| (x, y, z))))
}

pub(super) fn overlaps(a: Bounds, b: Bounds) -> bool {
    a.min.x <= b.max.x
        && b.min.x <= a.max.x
        && a.min.y <= b.max.y
        && b.min.y <= a.max.y
        && a.min.z <= b.max.z
        && b.min.z <= a.max.z
}

impl Grid {
    pub fn configure(&mut self, count: usize, size: f32) {
        if self.bounds.len() != count || self.size != size {
            *self = Self {
                size,
                bounds: vec![None; count],
                ..Self::default()
            };
        }
    }

    pub fn update(&mut self, id: usize, bounds: Bounds) {
        if self.bounds[id] == Some(bounds) {
            return;
        }
        if let Some(old) = self.bounds[id] {
            match range(old, self.size) {
                Some(r) => {
                    for cell in cells(r) {
                        if let Some(ids) = self.cells.get_mut(&cell) {
                            ids.retain(|&i| i != id);
                            if ids.is_empty() {
                                self.cells.remove(&cell);
                            }
                        }
                    }
                }
                None => {
                    self.large.remove(&id);
                }
            }
        }
        match range(bounds, self.size) {
            Some(r) => {
                for cell in cells(r) {
                    self.cells.entry(cell).or_default().push(id);
                }
            }
            None => {
                self.large.insert(id);
            }
        }
        self.bounds[id] = Some(bounds);
    }

    pub fn query(&self, id: usize, out: &mut Vec<usize>) {
        out.clear();
        let bounds = self.bounds[id].expect("indexed body");
        match range(bounds, self.size) {
            Some(r) => {
                for cell in cells(r) {
                    if let Some(ids) = self.cells.get(&cell) {
                        out.extend(ids);
                    }
                }
                out.extend(&self.large);
                out.sort_unstable();
                out.dedup();
            }
            // Large colliders use a bounded-memory fallback, never silent omission.
            None => out.extend(0..self.bounds.len()),
        }
        out.retain(|&other| other != id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use io_types::Vec3;

    #[test]
    fn grid_matches_brute_force_across_boundaries_moves_and_large_colliders() {
        let mut grid = Grid::default();
        grid.configure(151, 2.);
        let mut boxes = Vec::new();
        for i in 0..150 {
            let p = Vec3::new(
                (i % 11) as f32 * 1.9 - 10.,
                (i % 17) as f32 - 8.,
                (i % 7) as f32 - 3.,
            );
            boxes.push(Bounds {
                min: p,
                max: p + Vec3::new(2., 1., 1.),
            });
        }
        boxes.push(Bounds {
            min: Vec3::new(-100., -100., -1.),
            max: Vec3::new(100., 100., 0.),
        });
        let mut found = Vec::new();
        for pass in 0..2 {
            if pass == 1 {
                boxes[0] = boxes[70];
            }
            for (i, &b) in boxes.iter().enumerate() {
                grid.update(i, b);
            }
            for (i, &b) in boxes.iter().enumerate() {
                grid.query(i, &mut found);
                let actual: Vec<_> = found
                    .iter()
                    .copied()
                    .filter(|&j| overlaps(b, boxes[j]))
                    .collect();
                let expected: Vec<_> = boxes
                    .iter()
                    .enumerate()
                    .filter_map(|(j, &other)| (i != j && overlaps(b, other)).then_some(j))
                    .collect();
                assert_eq!(actual, expected);
            }
        }
    }

    #[test]
    fn dense_cube_search_is_local_in_all_three_dimensions() {
        let n = 20;
        let mut grid = Grid::default();
        grid.configure(n * n * n, 2.);
        for x in 0..n {
            for y in 0..n {
                for z in 0..n {
                    let p = Vec3::new(x as f32 * 1.002, y as f32 * 1.002, z as f32 * 1.002);
                    grid.update(
                        (x * n + y) * n + z,
                        Bounds {
                            min: p,
                            max: p + Vec3::new(1., 1., 1.),
                        },
                    );
                }
            }
        }
        let mut found = Vec::new();
        let mut checks = 0;
        for i in 0..n * n * n {
            grid.query(i, &mut found);
            checks += found.len();
        }
        assert!(checks < n * n * n * 70, "{checks} neighbor visits");
    }
}
