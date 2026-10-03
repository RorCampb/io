use super::*;
use crate::navigation::{Connection, Endpoints, Failure, Location, Waypoint};
use io_world::{Space, World, WorldView};
use std::sync::{atomic::AtomicUsize, Condvar};

#[test]
fn urgent_work_gets_more_slices_without_starving_routine_work() {
    let mut queue = VecDeque::from([(0, 0_u8), (1, 2), (2, 2), (3, 1)]);
    let mut counts = [0_i32; 4];
    for slice in 0..160 {
        let index = priority_index(queue.iter().map(|(_, p)| *p), slice);
        let job = queue.remove(index).unwrap();
        counts[job.0] += 1;
        queue.push_back(job);
    }
    assert!(counts.iter().all(|&c| c > 0), "{counts:?}");
    assert!(counts[1] > counts[0] && counts[2] > counts[0]);
    assert!((counts[1] - counts[2]).abs() <= 1);
}

#[test]
fn admission_fairness_does_not_alias_population_with_priority_cadence() {
    let mut order = AdmissionOrder::default();
    let mut counts = [0; 128];
    for tick in 1..=512 {
        let selected = order.select(128, tick, |i| {
            if i == 0 {
                NavigationPriority::Urgent
            } else {
                NavigationPriority::Routine
            }
        });
        counts[selected] += 1;
    }
    assert!(counts.iter().all(|&n| n >= 1));
    assert_eq!(counts[0], 385);
}

#[derive(Debug, Default)]
struct Gate {
    entered: AtomicUsize,
    open: Mutex<bool>,
    wake: Condvar,
    refinement: Mutex<Option<Arc<Gate>>>,
}
impl Gate {
    fn release(&self) {
        *self.open.lock().unwrap() = true;
        self.wake.notify_all();
    }
    fn wait(&self) {
        assert_eq!(thread::current().name(), Some("io-planning"));
        self.entered.fetch_add(1, Ordering::Release);
        // A finite fault injection: a failing test can always shut its worker down.
        let _ = self
            .wake
            .wait_timeout_while(self.open.lock().unwrap(), Duration::from_secs(2), |open| {
                !*open
            })
            .unwrap();
    }
}
#[derive(Clone, Debug)]
struct Line(Arc<Gate>);
fn point(n: u32) -> Vec3 {
    Vec3::new(n as f32, 0., 0.)
}
impl RouteProvider for Line {
    type Config = Arc<Gate>;
    type Node = u32;
    type Action = ();
    type Profile = u8;
    fn new(g: Self::Config) -> Result<Self, &'static str> {
        Ok(Self(g))
    }
    fn validate_profile(_: u8) -> Result<(), &'static str> {
        Ok(())
    }
    fn invalidate(&mut self) {}
    fn begin(
        &self,
        _: &dyn WorldView,
        profile: u8,
        a: Vec3,
        b: Vec3,
        _: Option<u64>,
    ) -> Result<Endpoints<u32, ()>, Failure> {
        self.0.wait();
        assert_ne!(profile, 99, "injected provider failure");
        Ok(Endpoints {
            start: Location {
                node: a.x as u32,
                position: a,
            },
            goal: b.x as u32,
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
    fn destination_available(&self, _: &dyn WorldView, _: u8, _: Option<u64>, _: Vec3) -> bool {
        true
    }
    fn neighbors(
        &mut self,
        _: &dyn WorldView,
        _: u8,
        from: Location<u32>,
        _: &mut Stats,
    ) -> Vec<Connection<u32, ()>> {
        vec![Connection {
            destination: Location {
                node: from.node + 1,
                position: point(from.node + 1),
            },
            action: (),
            cost: 1,
        }]
    }
    fn live_clear(
        &self,
        _: &dyn WorldView,
        _: u8,
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
        _: u8,
        _: Option<u64>,
        from: Vec3,
        to: Vec3,
        _: (),
    ) -> bool {
        if to.x - from.x > 1.5 {
            let gate = self.0.refinement.lock().unwrap().clone();
            if let Some(gate) = gate {
                gate.wait();
            }
        }
        true
    }
    fn automatic_completion(&self, _: ()) -> bool {
        self.0.refinement.lock().unwrap().is_some()
    }
}
fn job(actor: u64, attempt: u64, profile: u8, destination: u32) -> PlanJob<Line> {
    let world = World::new(Space::new(Vec3::new(20., 20., 20.)), vec![]);
    PlanJob {
        priority: NavigationPriority::Routine,
        key: PlanKey {
            ticket: NavigationTicket { actor, revision: 1 },
            attempt,
        },
        revision: world.navigation_revision(),
        start: point(0),
        goal: NavigationGoal::Position(point(destination)),
        agent: Agent::for_actor(actor, profile, point(destination)).unwrap(),
        world: Arc::new(world.snapshot()),
        familiar: vec![],
        horizon: 3.,
    }
}
fn until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !condition() {
        assert!(Instant::now() < deadline, "worker deadline exceeded");
        thread::sleep(Duration::from_millis(1));
    }
}
#[test]
fn blocked_provider_does_not_block_polling_and_queue_is_bounded() {
    let gate = Arc::new(Gate::default());
    let mut planner = Planner::new(PlanningMode::Background);
    assert!(planner.submit(job(1, 1, 0, 2), gate.clone()).unwrap());
    until(|| gate.entered.load(Ordering::Acquire) > 0);
    for actor in 2..=CAPACITY as u64 {
        assert!(planner.submit(job(actor, 1, 0, 2), gate.clone()).unwrap());
    }
    assert!(!planner.submit(job(33, 1, 0, 2), gate.clone()).unwrap());
    // The provider is still blocked, yet simulation-side polling returns.
    planner.poll();
    assert_eq!(planner.stats().completed, 0);
    assert_eq!(planner.stats().pending, CAPACITY);
    gate.release();
    until(|| {
        planner.poll();
        planner.stats().completed == CAPACITY as u64
    });
}

#[test]
fn inflight_priority_changes_do_not_restart_or_cancel_the_search() {
    let gate = Arc::new(Gate::default());
    let mut planner = Planner::new(PlanningMode::Background);
    let key = job(1, 1, 0, 20).key;
    planner.submit(job(1, 1, 0, 20), gate.clone()).unwrap();
    until(|| gate.entered.load(Ordering::Acquire) > 0);
    planner.prioritize(1, NavigationPriority::Urgent);
    assert_eq!(planner.inflight[&1].2.load(Ordering::Relaxed), 2);
    planner.prioritize(1, NavigationPriority::Routine);
    assert_eq!(planner.stats().priority_updates, 2);
    assert_eq!(planner.stats().submitted, 1);
    assert_eq!(planner.stats().cancelled, 0);
    assert!(planner.contains(key));
    gate.release();
    until(|| {
        planner.poll();
        planner.results.contains_key(&1)
    });
    assert!(planner.take(key).is_some());
}
#[test]
fn replaced_attempt_and_cancelled_objective_cannot_return_old_results() {
    let gate = Arc::new(Gate::default());
    let mut planner = Planner::new(PlanningMode::Background);
    let old = job(1, 1, 0, 2).key;
    planner.submit(job(1, 1, 0, 2), gate.clone()).unwrap();
    until(|| gate.entered.load(Ordering::Acquire) > 0);
    let current = job(1, 2, 0, 3).key;
    planner.submit(job(1, 2, 0, 3), gate.clone()).unwrap();
    gate.release();
    until(|| {
        planner.poll();
        planner.results.contains_key(&1)
    });
    assert!(planner.take(old).is_none());
    let result = planner.take(current).unwrap();
    assert_eq!(
        result.agent.route().unwrap().last().unwrap().position,
        point(3)
    );
    planner.submit(job(2, 1, 0, 2), gate.clone()).unwrap();
    planner.cancel(2);
    planner.poll();
    assert!(!planner.contains(job(2, 1, 0, 2).key));
    assert!(planner.take(job(2, 1, 0, 2).key).is_none());
    assert_eq!(planner.stats().pending, 0);
}
#[test]
fn published_clone_has_no_worker_and_simulated_clone_is_independent() {
    let gate = Arc::new(Gate::default());
    gate.release();
    let mut planner = Planner::new(PlanningMode::Background);
    planner.submit(job(1, 1, 0, 2), gate.clone()).unwrap();
    let mut clone = planner.clone();
    assert!(clone.runtime.is_none());
    assert!(clone.inflight.is_empty());
    clone.submit(job(1, 1, 0, 3), gate.clone()).unwrap();
    planner.cancel(1);
    until(|| {
        clone.poll();
        clone.results.contains_key(&1)
    });
    assert!(clone.take(job(1, 1, 0, 3).key).is_some());
}
#[test]
fn expansion_round_robin_and_provider_failure_do_not_starve_other_actors() {
    let gate = Arc::new(Gate::default());
    let mut planner = Planner::new(PlanningMode::Background);
    planner.submit(job(1, 1, 0, 100_000), gate.clone()).unwrap();
    until(|| gate.entered.load(Ordering::Acquire) > 0);
    planner.submit(job(2, 1, 99, 2), gate.clone()).unwrap();
    planner.submit(job(3, 1, 0, 2), gate.clone()).unwrap();
    gate.release();
    until(|| {
        planner.poll();
        planner.results.contains_key(&2) && planner.results.contains_key(&3)
    });
    assert_eq!(
        planner.take(job(2, 1, 99, 2).key).unwrap().agent.status(),
        Status::Failed
    );
    assert_eq!(
        planner.take(job(3, 1, 0, 2).key).unwrap().agent.status(),
        Status::Following
    );
    planner.cancel(1);
}

#[test]
fn refinement_runs_on_worker_is_cancellable_and_preserves_attempt_identity() {
    let gate = Arc::new(Gate::default());
    let refining = Arc::new(Gate::default());
    *gate.refinement.lock().unwrap() = Some(refining.clone());
    gate.release();
    let mut planner = Planner::new(PlanningMode::Background);
    let old = job(1, 1, 0, 12).key;
    planner.submit(job(1, 1, 0, 12), gate.clone()).unwrap();
    until(|| refining.entered.load(Ordering::Acquire) > 0);
    planner.poll(); // Returns while worker is inside a refinement geometry query.
    assert_eq!(planner.stats().completed, 0);
    let current = job(1, 2, 0, 9).key;
    planner.submit(job(1, 2, 0, 9), gate.clone()).unwrap();
    refining.release();
    until(|| {
        planner.poll();
        planner.results.contains_key(&1)
    });
    assert!(planner.take(old).is_none());
    let result = planner.take(current).unwrap();
    assert!(result.stats.refinement_queries > 0);
    assert!(result.agent.route().unwrap().len() <= 4);
    assert_eq!(
        result.agent.route().unwrap().last().unwrap().position,
        point(9)
    );
}
