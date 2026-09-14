//! Observational only: replay contacts on scratch bodies, never skip real work.
use super::{pair_mut, solver, Body, ColliderShape, Constraint};
use io_types::Vec3;

/// Reporting threshold in world units/second, never a solver stopping criterion.
pub const CORRECTION_SPEED_THRESHOLD: f64 = 1e-3;
pub const CORRECTION_SPEED_BOUNDS: [f64; 5] = [0., 1e-5, 1e-4, 1e-3, 1e-2];

#[derive(Clone, Copy, Debug, Default)]
pub struct ContactPassStats {
    /// Actual visits, summed across substeps (and across ticks when accumulated).
    pub visits: u64,
    pub correction_histogram: [u64; 6],
    pub end_pass_histogram: [u64; 6],
    pub small_then_large: u64,
    pub small_but_unsettled: u64,
    pub supporting_small: u64,
    pub invalid_samples: u64,
    pub max_correction_speed: f64,
    pub max_end_pass_correction_speed: f64,
}
impl ContactPassStats {
    pub fn accumulate(&mut self, other: &Self) {
        self.visits += other.visits;
        for i in 0..6 {
            self.correction_histogram[i] += other.correction_histogram[i];
            self.end_pass_histogram[i] += other.end_pass_histogram[i];
        }
        self.small_then_large += other.small_then_large;
        self.small_but_unsettled += other.small_but_unsettled;
        self.supporting_small += other.supporting_small;
        self.invalid_samples += other.invalid_samples;
        self.max_correction_speed = self.max_correction_speed.max(other.max_correction_speed);
        self.max_end_pass_correction_speed = self
            .max_end_pass_correction_speed
            .max(other.max_end_pass_correction_speed);
    }
}

#[derive(Default)]
pub struct ContactDiagnostics {
    pub passes: Vec<ContactPassStats>,
}
impl ContactDiagnostics {
    pub(super) fn begin_tick(&mut self, iterations: u32) {
        self.passes.clear();
        self.passes
            .resize(iterations as usize, ContactPassStats::default());
    }
}

fn length(v: Vec3) -> f64 {
    ((v.x as f64).powi(2) + (v.y as f64).powi(2) + (v.z as f64).powi(2)).sqrt()
}
fn radius(body: &Body) -> f64 {
    match body.collider.shape {
        ColliderShape::Sphere { radius } => radius as f64,
        ColliderShape::Box { half_extents } => length(half_extents),
    }
}
fn change(before: &Body, after: &Body, radius: f64) -> f64 {
    // Bound the induced velocity change at any point on this collider.
    length(after.velocity - before.velocity) + radius * length(after.omega - before.omega)
}
fn bucket(value: f64) -> usize {
    CORRECTION_SPEED_BOUNDS
        .iter()
        .position(|&bound| value <= bound)
        .unwrap_or(5)
}

pub(super) fn solve(
    bodies: &mut [Body],
    contacts: &mut [Constraint],
    prepared: &[solver::PreparedContact],
    diagnostics: &mut ContactDiagnostics,
) {
    assert_eq!(contacts.len(), prepared.len());
    let radii: Vec<_> = bodies.iter().map(radius).collect();
    let mut previous_small = vec![false; contacts.len()];
    for (iteration, stats) in diagnostics.passes.iter_mut().enumerate() {
        for (i, (c, p)) in contacts.iter_mut().zip(prepared).enumerate() {
            let (a, b) = pair_mut(bodies, c.a.index(), c.b.index());
            let (before_a, before_b) = (a.clone(), b.clone());
            solver::solve_contact(a, b, c, p);
            let da = change(&before_a, a, radii[c.a.index()]);
            let db = change(&before_b, b, radii[c.b.index()]);
            stats.visits += 1;
            if !da.is_finite() || !db.is_finite() {
                stats.invalid_samples += 1;
                previous_small[i] = false;
                continue;
            }
            let magnitude = da.max(db);
            let small = magnitude <= CORRECTION_SPEED_THRESHOLD;
            stats.correction_histogram[bucket(magnitude)] += 1;
            stats.max_correction_speed = stats.max_correction_speed.max(magnitude);
            stats.small_then_large += u64::from(iteration > 0 && previous_small[i] && !small);
            stats.supporting_small += u64::from(small && c.normal_impulse > 0.);
            previous_small[i] = small;
        }
        // Later contacts can undo earlier convergence. Replay from the final
        // pass state without mutating real bodies or accumulated impulses.
        for (i, (c, p)) in contacts.iter().zip(prepared).enumerate() {
            let (a, b) = (&bodies[c.a.index()], &bodies[c.b.index()]);
            let (mut scratch_a, mut scratch_b, mut scratch_c) = (a.clone(), b.clone(), c.clone());
            solver::solve_contact(&mut scratch_a, &mut scratch_b, &mut scratch_c, p);
            let da = change(a, &scratch_a, radii[c.a.index()]);
            let db = change(b, &scratch_b, radii[c.b.index()]);
            if !da.is_finite() || !db.is_finite() {
                stats.invalid_samples += 1;
                continue;
            }
            let magnitude = da.max(db);
            stats.end_pass_histogram[bucket(magnitude)] += 1;
            stats.max_end_pass_correction_speed =
                stats.max_end_pass_correction_speed.max(magnitude);
            stats.small_but_unsettled +=
                u64::from(previous_small[i] && magnitude > CORRECTION_SPEED_THRESHOLD);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn histogram_edges_are_inclusive_and_totals_accumulate() {
        for (value, expected) in [
            (0., 0),
            (1e-6, 1),
            (1e-5, 1),
            (1e-4, 2),
            (1e-3, 3),
            (1e-2, 4),
            (0.1, 5),
        ] {
            assert_eq!(bucket(value), expected);
        }
        let source = ContactPassStats {
            visits: 2,
            correction_histogram: [1, 0, 0, 0, 0, 1],
            end_pass_histogram: [0, 0, 0, 0, 1, 1],
            small_then_large: 1,
            small_but_unsettled: 1,
            supporting_small: 1,
            invalid_samples: 0,
            max_correction_speed: 2.,
            max_end_pass_correction_speed: 3.,
        };
        let mut total = source;
        total.accumulate(&source);
        assert_eq!(total.visits, 4);
        assert_eq!(total.correction_histogram, [2, 0, 0, 0, 0, 2]);
        assert_eq!(total.end_pass_histogram, [0, 0, 0, 0, 2, 2]);
        assert_eq!(
            (
                total.small_then_large,
                total.small_but_unsettled,
                total.supporting_small
            ),
            (2, 2, 2)
        );
        assert_eq!(total.max_correction_speed, 2.);
        assert_eq!(total.max_end_pass_correction_speed, 3.);
    }
}
