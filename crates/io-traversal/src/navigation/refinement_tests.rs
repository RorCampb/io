use super::*;
use crate::navigation::{Connection, Endpoints, Failure, Location, Stats};
use io_world::{Space, World};
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Travel,
    Sneak,
    Activate,
}
#[derive(Clone, Debug)]
struct Routes {
    calls: Arc<AtomicUsize>,
    clear: bool,
}
impl RouteProvider for Routes {
    type Config = (Arc<AtomicUsize>, bool);
    type Node = u32;
    type Action = Action;
    type Profile = u8;
    fn new((calls, clear): Self::Config) -> Result<Self, &'static str> {
        Ok(Self { calls, clear })
    }
    fn validate_profile(_: u8) -> Result<(), &'static str> {
        Ok(())
    }
    fn invalidate(&mut self) {}
    fn begin(
        &self,
        _: &dyn WorldView,
        _: u8,
        _: Vec3,
        _: Vec3,
        _: Option<u64>,
    ) -> Result<Endpoints<u32, Action>, Failure> {
        unreachable!()
    }
    fn destination_available(&self, _: &dyn WorldView, _: u8, _: Option<u64>, _: Vec3) -> bool {
        true
    }
    fn neighbors(
        &mut self,
        _: &dyn WorldView,
        _: u8,
        _: Location<u32>,
        _: &mut Stats,
    ) -> Vec<Connection<u32, Action>> {
        unreachable!()
    }
    fn live_clear(
        &self,
        _: &dyn WorldView,
        _: u8,
        _: Option<u64>,
        _: Vec3,
        _: Vec3,
        _: Action,
    ) -> bool {
        self.clear
    }
    fn automatic_completion(&self, action: Action) -> bool {
        action != Action::Activate
    }
    fn segment_clear(
        &self,
        _: &dyn WorldView,
        _: u8,
        _: Option<u64>,
        _: Vec3,
        _: Vec3,
        action: Action,
    ) -> bool {
        assert_ne!(action, Action::Activate);
        self.calls.fetch_add(1, Ordering::Relaxed);
        self.clear
    }
}
fn fixture(clear: bool, actions: &[Action]) -> (World, Navigation<Routes>, Agent<Routes>) {
    let world = World::new(Space::new(Vec3::new(100., 10., 10.)), vec![]);
    let nav = Navigation::new((Arc::new(AtomicUsize::new(0)), clear)).unwrap();
    let mut agent = Agent::for_actor(1, 0, Vec3::new(actions.len() as f32, 0., 0.)).unwrap();
    agent.revision = Some(world.navigation_revision());
    agent.state = State::Route {
        steps: actions
            .iter()
            .enumerate()
            .map(|(i, &action)| Waypoint {
                position: Vec3::new(i as f32 + 1., 0., 0.),
                action,
            })
            .collect::<Vec<_>>()
            .into(),
        next: 0,
        refined: false,
    };
    (world, nav, agent)
}

#[test]
fn bounded_stage_preserves_raw_route_until_atomic_commit_and_reuses_work() {
    let (world, mut nav, mut agent) = fixture(true, &[Action::Travel; 12]);
    let original = agent.route().unwrap().to_vec();
    let mut stage = RouteRefinement::default();
    loop {
        let before = nav.stats.refinement_queries;
        let result = stage
            .run(RefineRequest {
                navigation: &mut nav,
                world: &world,
                agent: &mut agent,
                start: Vec3::default(),
                horizon: 3.,
            })
            .unwrap();
        assert!(nav.stats.refinement_queries - before <= 1);
        match result {
            RefineProgress::Pending => assert_eq!(agent.route().unwrap(), original),
            RefineProgress::Complete => break,
        }
    }
    assert_eq!(
        agent.route().unwrap().len(),
        2,
        "collinear intermediate stops are merged"
    );
    assert_eq!(agent.route().unwrap().last(), original.last());
    let checks = nav.stats.refinement_queries;
    let calls = nav.provider.calls.load(Ordering::Relaxed);
    for _ in 0..144 {
        nav.refine_route(&world, &mut agent, Vec3::new(0.2, 0., 0.), 3.)
            .unwrap();
        assert!(nav
            .prepared_target(&mut agent, Vec3::new(0.2, 0., 0.), 3.)
            .is_some());
    }
    assert_eq!(nav.stats.refinement_queries, checks);
    assert_eq!(nav.provider.calls.load(Ordering::Relaxed), calls);
}

#[test]
fn failed_shortcuts_retain_planned_edges_and_discrete_boundaries() {
    let actions = [
        Action::Travel,
        Action::Travel,
        Action::Activate,
        Action::Activate,
        Action::Sneak,
        Action::Sneak,
        Action::Travel,
        Action::Travel,
    ];
    for clear in [false, true] {
        let (world, mut nav, mut agent) = fixture(clear, &actions);
        let raw = agent.route().unwrap().to_vec();
        nav.refine_route(&world, &mut agent, Vec3::default(), 30.)
            .unwrap();
        let route = agent.route().unwrap();
        if clear {
            assert_eq!(
                route.iter().map(|w| w.action).collect::<Vec<_>>(),
                vec![
                    Action::Travel,
                    Action::Activate,
                    Action::Activate,
                    Action::Sneak,
                    Action::Travel
                ]
            );
        } else {
            assert_eq!(route, raw);
        }
        assert!(route.contains(&raw[2]) && route.contains(&raw[3]));
        assert_eq!(route.last(), raw.last());
    }
}

#[test]
fn pending_stage_rejects_changed_inputs_without_replacing_route() {
    let (world, mut nav, mut agent) = fixture(false, &[Action::Travel; 12]);
    let mut stage = RouteRefinement::default();
    assert_eq!(
        stage
            .run(RefineRequest {
                navigation: &mut nav,
                world: &world,
                agent: &mut agent,
                start: Vec3::default(),
                horizon: 3.
            })
            .unwrap(),
        RefineProgress::Pending
    );
    let raw = agent.route().unwrap().to_vec();
    assert!(stage
        .run(RefineRequest {
            navigation: &mut nav,
            world: &world,
            agent: &mut agent,
            start: Vec3::new(0.1, 0., 0.),
            horizon: 3.
        })
        .is_err());
    assert_eq!(agent.route().unwrap(), raw);
}

#[test]
fn prepared_selection_never_cuts_a_corner_reversal_or_action_boundary() {
    let (world, mut nav, mut agent) = fixture(false, &[Action::Travel; 3]);
    if let State::Route { steps, .. } = &mut agent.state {
        Arc::make_mut(steps)[1].position = Vec3::new(1., 1., 0.);
    }
    nav.refine_route(&world, &mut agent, Vec3::default(), 3.)
        .unwrap();
    assert_eq!(
        nav.prepared_target(&mut agent, Vec3::default(), 3.)
            .unwrap()
            .position,
        Vec3::new(1., 0., 0.)
    );
    assert_eq!(
        nav.prepared_target(&mut agent, Vec3::new(0.995, 0., 0.), 3.)
            .unwrap()
            .position,
        Vec3::new(1., 0., 0.),
        "near a turn is not proof of corner clearance"
    );
    let (world, mut nav, mut agent) = fixture(false, &[Action::Travel, Action::Sneak]);
    nav.refine_route(&world, &mut agent, Vec3::default(), 3.)
        .unwrap();
    assert_eq!(
        nav.prepared_target(&mut agent, Vec3::default(), 3.)
            .unwrap()
            .action,
        Action::Travel
    );
    let (world, mut nav, mut agent) = fixture(false, &[Action::Travel; 2]);
    if let State::Route { steps, .. } = &mut agent.state {
        Arc::make_mut(steps)[1].position.x = -1.;
    }
    nav.refine_route(&world, &mut agent, Vec3::default(), 3.)
        .unwrap();
    assert_eq!(
        nav.prepared_target(&mut agent, Vec3::default(), 3.)
            .unwrap()
            .position
            .x,
        1.
    );
}

#[test]
fn handoff_rejects_stale_geometry_profile_and_moved_discrete_takeoff() {
    let (world, mut nav, mut agent) = fixture(true, &[Action::Activate, Action::Travel]);
    let revision = world.navigation_revision();
    for (snapshot_revision, current_profile, position) in [
        (revision.wrapping_add(1), 0, Vec3::default()),
        (revision, 1, Vec3::default()),
        (revision, 0, Vec3::new(0.1, 0., 0.)),
    ] {
        assert!(!RouteHandoff
            .run(HandoffRequest {
                navigation: &mut nav,
                world: &world,
                agent: &mut agent,
                planned_start: Vec3::default(),
                position,
                horizon: 3.,
                snapshot_revision,
                current_profile
            })
            .unwrap());
    }
    assert!(RouteHandoff
        .run(HandoffRequest {
            navigation: &mut nav,
            world: &world,
            agent: &mut agent,
            planned_start: Vec3::default(),
            position: Vec3::default(),
            horizon: 3.,
            snapshot_revision: revision,
            current_profile: 0
        })
        .unwrap());
}
