//! Bounded approach destinations. A failed endpoint is not evidence that the target is unreachable.
use crate::navigation::{Agent, Navigation, RouteProvider, Status};
use io_types::Vec3;
use io_world::WorldView;
use std::collections::VecDeque;

#[derive(Clone, Debug)]
struct Candidates {
    target: Vec3,
    revision: u64,
    remaining: VecDeque<Vec3>,
    selected: Option<Vec3>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::navigation::{Connection, Endpoints, Failure, Location, Stats, Waypoint};
    use io_world::{Space, World};
    #[derive(Clone, Debug)]
    struct TestRoutes;
    impl RouteProvider for TestRoutes {
        type Config = ();
        type Profile = ();
        type Action = ();
        type Node = ();
        fn new(_: ()) -> Result<Self, &'static str> {
            Ok(Self)
        }
        fn validate_profile(_: ()) -> Result<(), &'static str> {
            Ok(())
        }
        fn invalidate(&mut self) {}
        fn begin(
            &self,
            w: &dyn WorldView,
            p: (),
            a: Vec3,
            b: Vec3,
            id: Option<u64>,
        ) -> Result<Endpoints<(), ()>, Failure> {
            if !self.destination_available(w, p, id, b) {
                return Err(Failure::Unreachable);
            }
            Ok(Endpoints {
                start: Location {
                    node: (),
                    position: a,
                },
                goal: (),
                first: Waypoint {
                    position: a,
                    action: (),
                },
                last: Waypoint {
                    position: b,
                    action: (),
                },
            })
        }
        fn destination_available(&self, _: &dyn WorldView, _: (), _: Option<u64>, b: Vec3) -> bool {
            b.x.abs() < 10. && b.y.abs() < 10.
        }
        fn neighbors(
            &mut self,
            _: &dyn WorldView,
            _: (),
            _: Location<()>,
            _: &mut Stats,
        ) -> Vec<Connection<(), ()>> {
            vec![]
        }
        fn live_clear(
            &self,
            _: &dyn WorldView,
            _: (),
            _: Option<u64>,
            _: Vec3,
            _: Vec3,
            _: (),
        ) -> bool {
            true
        }
        fn segment_clear(
            &self,
            _: &dyn WorldView,
            _: (),
            _: Option<u64>,
            _: Vec3,
            _: Vec3,
            _: (),
        ) -> bool {
            true
        }
        fn offset_destination(
            &self,
            _: &dyn WorldView,
            _: (),
            _: Option<u64>,
            r: Vec3,
            o: [f32; 2],
        ) -> Option<Vec3> {
            Some(r + Vec3::new(o[0], o[1], 0.))
        }
    }

    #[test]
    fn selection_is_stable_and_failed_routes_advance_to_other_candidates() {
        let world = World::new(Space::new(Vec3::new(20., 20., 10.)), vec![]);
        let mut nav = Navigation::<TestRoutes>::new(()).unwrap();
        let profile = ();
        let target = Vec3::default();
        let start = Vec3::new(0., 5., 0.);
        let mut agent = Agent::new(profile, target).unwrap();
        let mut approach = Approach::default();
        let first = approach
            .destination(&nav, &world, &agent, start, target, 1.)
            .unwrap();
        agent.set_goal(first).unwrap();
        nav.advance(&world, &mut agent, start, 4096);
        assert_eq!(agent.status(), Status::Following);
        assert_eq!(
            approach.destination(&nav, &world, &agent, Vec3::new(2., 2., 0.), target, 1.),
            Some(first)
        );
        // Represent a completed, failed full-route search; endpoint availability alone isn't enough.
        agent.set_goal(Vec3::new(20., 20., 0.)).unwrap();
        nav.advance(&world, &mut agent, start, 4096);
        assert_eq!(agent.status(), Status::Unreachable);
        let next = approach
            .destination(&nav, &world, &agent, start, target, 1.)
            .unwrap();
        assert_ne!(first, next);
        assert!(nav.destination_available(&world, &agent, next));
        approach.clear();
        assert!(!approach.has_destination());
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Approach {
    candidates: Option<Candidates>,
}
impl Approach {
    pub fn clear(&mut self) {
        self.candidates = None;
    }
    pub fn has_destination(&self) -> bool {
        self.candidates
            .as_ref()
            .is_some_and(|c| c.selected.is_some())
    }
    pub fn destination<P: RouteProvider>(
        &mut self,
        nav: &Navigation<P>,
        world: &dyn WorldView,
        agent: &Agent<P>,
        position: Vec3,
        target: Vec3,
        radius: f32,
    ) -> Option<Vec3> {
        let revision = nav.revision(world);
        let changed = self.candidates.as_ref().is_none_or(|c| {
            let delta = target - c.target;
            c.revision != revision
                || delta.dot(delta) > nav.goal_tolerance().powi(2)
                || (c.selected.is_none() && c.remaining.is_empty())
        });
        if changed {
            let delta = position - target;
            let preferred = delta.y.atan2(delta.x);
            let mut points: Vec<_> = (0..16)
                .filter_map(|i| {
                    let angle = preferred + i as f32 * std::f32::consts::TAU / 16.;
                    nav.offset_destination(
                        world,
                        agent,
                        target,
                        [angle.cos() * radius, angle.sin() * radius],
                    )
                })
                .collect();
            points.sort_by(|a, b| {
                (*a - position)
                    .dot(*a - position)
                    .total_cmp(&(*b - position).dot(*b - position))
            });
            self.candidates = Some(Candidates {
                target,
                revision,
                remaining: points.into(),
                selected: None,
            });
        }
        let candidates = self.candidates.as_mut()?;
        if let Some(goal) = candidates.selected {
            if !matches!(agent.status(), Status::Unreachable | Status::Blocked)
                && nav.destination_available(world, agent, goal)
            {
                return Some(goal);
            }
            candidates.selected = None;
        }
        while let Some(goal) = candidates.remaining.pop_front() {
            if nav.destination_available(world, agent, goal) {
                candidates.selected = Some(goal);
                return Some(goal);
            }
        }
        None
    }
}
