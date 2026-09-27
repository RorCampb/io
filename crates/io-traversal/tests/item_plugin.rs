//! Core-only consumer: a model-less spherical cargo lift, with no locomotion plugin dependency.
use io_game::{GamePlugin, PluginInfo, PluginWorld, Session, Tick};
use io_traversal::navigation::{
    Agent, Connection, Endpoints, Failure, Location, Navigation, RouteProvider, Stats, Waypoint,
};
use io_traversal::*;
use io_types::{Envelope, MessageId, Vec3};
use io_world::{
    cast_sphere, BodyKind, Collider, ColliderShape, CommandOutcome, Item, PhysicsBody, Space,
    World, WorldCommand, WorldView,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LiftAction {
    Calibrate,
    Translate,
}
#[derive(Clone, Copy, Debug, PartialEq)]
struct Capability {
    item: u64,
    vertical: bool,
    invalid_cost: bool,
}
#[derive(Clone, Debug)]
struct LiftRoutes;
fn location(z: i32) -> Location<i32> {
    Location {
        node: z,
        position: Vec3::new(0., 0., z as f32),
    }
}
impl RouteProvider for LiftRoutes {
    type Config = ();
    type Node = i32;
    type Action = LiftAction;
    type Profile = Capability;
    fn new(_: ()) -> Result<Self, &'static str> {
        Ok(Self)
    }
    fn validate_profile(p: Capability) -> Result<(), &'static str> {
        if p.item == 0 {
            Err("missing item")
        } else {
            Ok(())
        }
    }
    fn invalidate(&mut self) {}
    fn revision(&self, w: &dyn WorldView) -> u64 {
        // Only static geometry is persistent input for this particular provider.
        // The executor rechecks all colliders immediately before movement.
        use std::hash::{Hash, Hasher};
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        for i in w.items().iter().filter(|i| {
            i.physics_body
                .as_ref()
                .is_some_and(|b| b.kind == BodyKind::Static)
        }) {
            format!("{i:?}").hash(&mut hash);
        }
        hash.finish()
    }
    fn begin(
        &self,
        w: &dyn WorldView,
        p: Capability,
        a: Vec3,
        b: Vec3,
        _: Option<u64>,
    ) -> Result<Endpoints<i32, LiftAction>, Failure> {
        if !self.destination_available(w, p, None, a) || !self.destination_available(w, p, None, b)
        {
            return Err(Failure::Unreachable);
        }
        Ok(Endpoints {
            start: location(a.z.round() as i32),
            goal: b.z.round() as i32,
            first: Waypoint {
                position: a,
                action: LiftAction::Calibrate,
            },
            last: Waypoint {
                position: b,
                action: LiftAction::Calibrate,
            },
        })
    }
    fn destination_available(
        &self,
        _: &dyn WorldView,
        _: Capability,
        _: Option<u64>,
        p: Vec3,
    ) -> bool {
        p.finite()
            && p.x == 0.
            && p.y == 0.
            && (0. ..=8.).contains(&p.z)
            && (p.z - p.z.round()).abs() < 0.001
    }
    fn neighbors(
        &mut self,
        w: &dyn WorldView,
        p: Capability,
        from: Location<i32>,
        _: &mut Stats,
    ) -> Vec<Connection<i32, LiftAction>> {
        if !p.vertical {
            return vec![];
        }
        [from.node - 1, from.node + 1]
            .into_iter()
            .filter(|n| (0..=8).contains(n))
            .filter(|n| {
                self.live_clear(
                    w,
                    p,
                    Some(p.item),
                    from.position,
                    location(*n).position,
                    LiftAction::Translate,
                )
            })
            .map(|n| Connection {
                destination: location(n),
                action: LiftAction::Translate,
                cost: if p.invalid_cost { 0 } else { 1000 },
            })
            .collect()
    }
    fn live_clear(
        &self,
        w: &dyn WorldView,
        p: Capability,
        _: Option<u64>,
        a: Vec3,
        b: Vec3,
        _: LiftAction,
    ) -> bool {
        cast_sphere(w, a, b, 0.2, Some(p.item)).is_ok_and(|t| t == 1.)
    }
    fn segment_clear(
        &self,
        w: &dyn WorldView,
        p: Capability,
        id: Option<u64>,
        a: Vec3,
        b: Vec3,
        action: LiftAction,
    ) -> bool {
        self.live_clear(w, p, id, a, b, action)
    }
}

#[derive(Clone, Copy, Debug)]
struct LiftFeedback {
    position: Vec3,
    calibrations: u32,
}
#[derive(Clone, Debug)]
struct Lift {
    id: u64,
    capability: Capability,
    step: Option<Waypoint<LiftAction>>,
    elapsed: f32,
    calibrations: u32,
    blocked: bool,
    fail_execution: bool,
}
impl TraversalExecutor for Lift {
    type Routes = LiftRoutes;
    type Feedback = LiftFeedback;
    type Event = LiftAction;
    fn item(&self) -> u64 {
        self.id
    }
    fn validate(&self, w: &dyn WorldView) -> Result<(), Error> {
        let i = w.item(self.id).ok_or(Error::InvalidWorld)?;
        if i.physics_body
            .as_ref()
            .is_none_or(|b| b.kind != BodyKind::Kinematic)
            || !matches!(i.collider.map(|c|c.shape),Some(ColliderShape::Sphere{radius}) if radius==0.2)
        {
            return Err(Error::InvalidWorld);
        }
        Ok(())
    }
    fn profile(&self, _: &dyn WorldView) -> Result<Capability, Error> {
        Ok(self.capability)
    }
    fn feedback(&self, w: &dyn WorldView) -> Result<LiftFeedback, Error> {
        Ok(LiftFeedback {
            position: w.item(self.id).ok_or(Error::InvalidWorld)?.transform.anchor,
            calibrations: self.calibrations,
        })
    }
    fn cancel(&mut self) {
        self.step = None;
        self.elapsed = 0.;
        self.blocked = false;
    }
    fn begin<E>(
        &mut self,
        _: &mut PluginWorld<E>,
        step: Waypoint<LiftAction>,
    ) -> Result<(), Error> {
        self.step = Some(step);
        self.elapsed = 0.;
        Ok(())
    }
    fn before_step<E>(&mut self, w: &mut PluginWorld<E>, dt: f32) -> Result<(), Error> {
        let current = w.item(self.id).ok_or(Error::InvalidWorld)?.transform.anchor;
        let target = match self.step {
            Some(Waypoint {
                position,
                action: LiftAction::Translate,
            }) => {
                let dz = (position.z - current.z).clamp(-2. * dt, 2. * dt);
                let next = current + Vec3::new(0., 0., dz);
                if cast_sphere(w, current, next, 0.2, Some(self.id))
                    .map_err(|_| Error::InvalidWorld)?
                    < 1.
                {
                    self.blocked = true;
                    current
                } else {
                    next
                }
            }
            _ => current,
        };
        match w.apply(WorldCommand::SetKinematicTarget {
            target: self.id,
            anchor: target,
            rotation: Default::default(),
        }) {
            CommandOutcome::Applied | CommandOutcome::Unchanged => Ok(()),
            CommandOutcome::Rejected(_) => Err(Error::InvalidWorld),
        }
    }
    fn execute<E>(
        &mut self,
        w: &mut PluginWorld<E>,
        step: Option<Waypoint<LiftAction>>,
        dt: f32,
        _: &mut Navigation<LiftRoutes>,
        _: &Agent<LiftRoutes>,
    ) -> Result<Execution<LiftFeedback, LiftAction>, Error> {
        let mut progress = Progress::Running;
        let mut events = vec![];
        self.elapsed += dt;
        if self.fail_execution && step.is_some() {
            progress = Progress::Failed;
        } else if self.blocked {
            progress = Progress::Blocked;
        } else if let Some(s) = step {
            let complete = match s.action {
                LiftAction::Calibrate => self.elapsed >= 0.1,
                LiftAction::Translate => {
                    (self.feedback(w)?.position.z - s.position.z).abs() < 0.001
                }
            };
            if complete {
                if s.action == LiftAction::Calibrate {
                    self.calibrations += 1;
                }
                progress = Progress::Complete;
                events.push(s.action);
            }
        }
        Ok(Execution {
            feedback: self.feedback(w)?,
            events,
            progress,
        })
    }
}

#[derive(Clone, Debug)]
struct Workshop {
    movement: Movement<Lift>,
}
impl GamePlugin for Workshop {
    type Command = NavigationRequest;
    type Event = MovementEvent<LiftAction>;
    type Error = Error;
    fn info(&self) -> PluginInfo {
        PluginInfo {
            id: "lift-demo",
            version: 1,
        }
    }
    fn validate(&self, w: &dyn WorldView) -> Result<(), Error> {
        self.movement.validate(w)
    }
    fn active_items(&self) -> Vec<u64> {
        self.movement.active_items()
    }
    fn command(&mut self, _: &mut PluginWorld<Self::Event>, c: Self::Command) -> Result<(), Error> {
        self.movement
            .request(Envelope::new(MessageId(1), c))
            .payload
            .map(|_| ())
            .map_err(|_| Error::InvalidInput)
    }
    fn before_step(&mut self, w: &mut PluginWorld<Self::Event>, t: Tick) -> Result<(), Error> {
        self.movement.before_step(w, t.seconds())
    }
    fn update(&mut self, w: &mut PluginWorld<Self::Event>, t: Tick) -> Result<(), Error> {
        let frame = self.movement.update(w, t.seconds())?;
        for e in frame.events {
            w.emit(e);
        }
        Ok(())
    }
}
fn fixture(vertical: bool, invalid_cost: bool) -> (World, Session<Workshop>) {
    let mut item = Item {
        id: 1,
        physics_body: Some(PhysicsBody::new(BodyKind::Kinematic)),
        collider: Some(Collider::new(ColliderShape::Sphere { radius: 0.2 })),
        ..Default::default()
    };
    item.transform.anchor = Vec3::new(0., 0., 1.);
    let world = World::new(Space::new(Vec3::new(10., 10., 10.)), vec![item]);
    let executor = Lift {
        id: 1,
        capability: Capability {
            item: 1,
            vertical,
            invalid_cost,
        },
        step: None,
        elapsed: 0.,
        calibrations: 0,
        blocked: false,
        fail_execution: false,
    };
    let movement = Movement::new(
        (),
        2,
        vec![ActorBinding {
            executor,
            familiar_points: vec![],
            look_ahead: 3.,
        }],
        &world,
    )
    .unwrap();
    let session = Session::register(Workshop { movement }, &world).unwrap();
    (world, session)
}
fn start(w: &mut World, g: &mut Session<Workshop>) {
    g.command(
        w,
        NavigationRequest::Start {
            actor: 1,
            goal: NavigationGoal::Position(Vec3::new(0., 0., 3.)),
        },
    )
    .unwrap();
}

fn background_fixture() -> (World, Session<Workshop>) {
    let (world, game) = fixture(true, false);
    let movement = game
        .movement
        .clone()
        .with_planning(PlanningMode::Background);
    let game = Session::register(Workshop { movement }, &world).unwrap();
    (world, game)
}

#[test]
fn item_population_is_independent_of_planning_queue_capacity() {
    let (w, g) = fixture(true, false);
    let mut items = Vec::new();
    let mut bindings = Vec::new();
    for id in 1..=128 {
        let mut item = w.item(1).unwrap().clone();
        item.id = id;
        item.transform.anchor.x = id as f32 * 2.;
        items.push(item);
        let mut executor = g.movement.executor(1).unwrap().clone();
        executor.id = id;
        executor.capability.item = id;
        bindings.push(ActorBinding {
            executor,
            familiar_points: vec![],
            look_ahead: 3.,
        });
    }
    let world = World::new(Space::new(Vec3::new(300., 10., 10.)), items);
    let movement = Movement::new((), 2, bindings, &world)
        .unwrap()
        .with_planning(PlanningMode::Background);
    assert_eq!(movement.active_items().len(), 128);
    assert_eq!(movement.planning_stats().pending, 0);
    movement.validate(&world).unwrap();
}
fn background_until(
    w: &mut World,
    g: &mut Session<Workshop>,
    done: impl Fn(&Session<Workshop>) -> bool,
) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !done(g) {
        assert!(
            std::time::Instant::now() < deadline,
            "{:?}",
            g.movement.actor(1)
        );
        g.step(w, &[], 1. / 60.).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}
#[test]
fn background_route_executes_the_same_non_humanoid_contract() {
    let (mut w, mut g) = background_fixture();
    start(&mut w, &mut g);
    background_until(&mut w, &mut g, |g| {
        g.movement.actor(1).unwrap().status == NavigationStatus::Arrived
    });
    assert_eq!(g.movement.actor(1).unwrap().execution.calibrations, 2);
    assert!((w.item(1).unwrap().transform.anchor.z - 3.).abs() < 0.001);
    assert_eq!(g.movement.planning_stats().accepted, 1);
}

#[test]
fn background_endpoint_failure_is_accepted_without_a_search_revision() {
    let (mut world, mut game) = background_fixture();
    game.command(
        &mut world,
        NavigationRequest::Start {
            actor: 1,
            goal: NavigationGoal::Approach {
                position: Vec3::new(0., 0., 3.),
                distance: 1.,
                repath_seconds: 1.,
            },
        },
    )
    .unwrap();
    // This provider has no approach-offset capability. Failure is an authoritative
    // result even though no graph search was initialized on the worker.
    background_until(&mut world, &mut game, |g| {
        g.movement.planning_stats().accepted > 0
    });
    assert_eq!(game.movement.planning_stats().rejected, 0);
    assert_eq!(game.movement.actor(1).unwrap().execution.calibrations, 0);
}
#[test]
fn background_clone_resubmits_without_stealing_original_job() {
    let (mut w, mut g) = background_fixture();
    start(&mut w, &mut g);
    g.step(&mut w, &[], 1. / 60.).unwrap();
    let mut other = g.clone();
    let mut other_world = World::new(w.space().clone(), w.items().to_vec());
    let ticket = g.movement.actor(1).unwrap().ticket;
    g.command(&mut w, NavigationRequest::Cancel { ticket })
        .unwrap();
    background_until(&mut other_world, &mut other, |g| {
        g.movement.actor(1).unwrap().status == NavigationStatus::Arrived
    });
    g.step(&mut w, &[], 1. / 60.).unwrap();
    assert_eq!(g.movement.actor(1).unwrap().status, NavigationStatus::Idle);
    assert_eq!(w.item(1).unwrap().transform.anchor.z, 1.);
}
#[test]
fn background_retarget_preserves_execution_then_hands_off_at_safe_start() {
    let (mut w, mut g) = background_fixture();
    start(&mut w, &mut g);
    background_until(&mut w, &mut g, |g| {
        g.movement.actor(1).unwrap().execution.position.z > 1.1
    });
    let old_ticket = g.movement.actor(1).unwrap().ticket;
    let old_position = w.item(1).unwrap().transform.anchor.z;
    g.command(
        &mut w,
        NavigationRequest::Start {
            actor: 1,
            goal: NavigationGoal::Position(Vec3::new(0., 0., 4.)),
        },
    )
    .unwrap();
    g.step(&mut w, &[], 1. / 60.).unwrap();
    let f = g.movement.actor(1).unwrap();
    assert_eq!(f.status, NavigationStatus::Planning);
    assert_eq!(f.execution_ticket, old_ticket);
    assert!(w.item(1).unwrap().transform.anchor.z > old_position);
    background_until(&mut w, &mut g, |g| {
        g.movement.actor(1).unwrap().status == NavigationStatus::Arrived
    });
    assert!((w.item(1).unwrap().transform.anchor.z - 4.).abs() < 0.001);
    let f = g.movement.actor(1).unwrap();
    assert_ne!(f.ticket, old_ticket);
    assert_eq!(f.ticket, f.execution_ticket);
}
#[test]
fn geometry_change_during_background_search_never_installs_obsolete_route() {
    let (mut w, mut g) = background_fixture();
    start(&mut w, &mut g);
    g.step(&mut w, &[], 1. / 60.).unwrap();
    let mut items = w.items().to_vec();
    let mut wall = Item {
        id: 2,
        physics_body: Some(PhysicsBody::new(BodyKind::Static)),
        collider: Some(Collider::new(ColliderShape::Box {
            half_extents: Vec3::new(2., 2., 0.1),
        })),
        ..Default::default()
    };
    wall.transform.anchor = Vec3::new(0., 0., 2.);
    items.push(wall);
    w = World::new(w.space().clone(), items);
    background_until(&mut w, &mut g, |g| {
        g.movement.actor(1).unwrap().status == NavigationStatus::Unreachable
    });
    assert_eq!(w.item(1).unwrap().transform.anchor.z, 1.);
    assert!(g.movement.planning_stats().submitted >= 2);
}

#[test]
fn non_humanoid_item_uses_physics_and_waits_for_explicit_action_completion() {
    for hz in [30, 60, 144] {
        let (mut w, mut g) = fixture(true, false);
        assert!(w.item(1).unwrap().character_body.is_none());
        assert!(w.item(1).unwrap().animation.is_none());
        start(&mut w, &mut g);
        g.step(&mut w, &[], 1. / hz as f32).unwrap();
        assert_ne!(
            g.movement.actor(1).unwrap().status,
            NavigationStatus::Arrived
        );
        assert_eq!(g.movement.actor(1).unwrap().execution.calibrations, 0);
        for _ in 0..hz * 4 {
            g.step(&mut w, &[], 1. / hz as f32).unwrap();
        }
        let f = g.movement.actor(1).unwrap();
        assert_eq!(f.status, NavigationStatus::Arrived);
        assert_eq!(f.execution.calibrations, 2);
        assert!((f.execution.position.z - 3.).abs() < 0.001);
        assert_eq!(w.item(1).unwrap().simulated_ticks, 1 + hz * 4);
    }
}
#[test]
fn capability_and_invalid_provider_results_are_not_confused() {
    for (vertical, invalid, expected) in [
        (false, false, NavigationStatus::Unreachable),
        (true, true, NavigationStatus::Failed),
    ] {
        let (mut w, mut g) = fixture(vertical, invalid);
        start(&mut w, &mut g);
        for _ in 0..20 {
            g.step(&mut w, &[], 1. / 60.).unwrap();
        }
        assert_eq!(g.movement.actor(1).unwrap().status, expected);
        assert_eq!(w.item(1).unwrap().transform.anchor.z, 1.);
    }
}
#[test]
fn cancel_stops_physical_target_and_snapshots_do_not_share_executor_state() {
    let (mut w, mut g) = fixture(true, false);
    start(&mut w, &mut g);
    for _ in 0..20 {
        g.step(&mut w, &[], 1. / 60.).unwrap();
    }
    let before = w.item(1).unwrap().transform.anchor;
    let mut other_world = World::new(w.space().clone(), w.items().to_vec());
    let mut other_game = g.clone();
    let ticket = g.movement.actor(1).unwrap().ticket;
    g.command(&mut w, NavigationRequest::Cancel { ticket })
        .unwrap();
    for _ in 0..60 {
        g.step(&mut w, &[], 1. / 60.).unwrap();
        other_game.step(&mut other_world, &[], 1. / 60.).unwrap();
    }
    assert_eq!(w.item(1).unwrap().transform.anchor, before);
    assert!(other_world.item(1).unwrap().transform.anchor.z > before.z + 0.5);
    assert_eq!(g.movement.actor(1).unwrap().status, NavigationStatus::Idle);
    assert!(g
        .command(&mut w, NavigationRequest::Cancel { ticket })
        .is_err());
}
#[test]
fn physical_obstacle_rejects_vertical_route() {
    let (mut w, mut g) = fixture(true, false);
    let mut items = w.items().to_vec();
    let mut wall = Item {
        id: 2,
        physics_body: Some(PhysicsBody::new(BodyKind::Static)),
        collider: Some(Collider::new(ColliderShape::Box {
            half_extents: Vec3::new(2., 2., 0.1),
        })),
        ..Default::default()
    };
    wall.transform.anchor = Vec3::new(0., 0., 2.);
    items.push(wall);
    w = World::new(w.space().clone(), items);
    start(&mut w, &mut g);
    for _ in 0..30 {
        g.step(&mut w, &[], 1. / 60.).unwrap();
    }
    assert_eq!(
        g.movement.actor(1).unwrap().status,
        NavigationStatus::Unreachable
    );
    assert_eq!(w.item(1).unwrap().transform.anchor.z, 1.);
}

#[test]
fn replacing_item_capabilities_is_validated_revisioned_and_replans_the_objective() {
    let (mut world, game) = fixture(true, false);
    let mut movement = game.movement.clone();
    let ticket = movement
        .request(Envelope::new(
            MessageId(7),
            NavigationRequest::Start {
                actor: 1,
                goal: NavigationGoal::Position(Vec3::new(0., 0., 3.)),
            },
        ))
        .payload
        .unwrap();
    let replacement = |id, vertical| Lift {
        id,
        capability: Capability {
            item: id,
            vertical,
            invalid_cost: false,
        },
        step: None,
        elapsed: 0.,
        calibrations: 0,
        blocked: false,
        fail_execution: false,
    };
    assert_eq!(
        movement.replace_executor(ticket, replacement(2, false), &world),
        Err(NavigationError::InvalidConfiguration)
    );
    assert_eq!(movement.actor(1).unwrap().ticket, ticket);
    let new_ticket = movement
        .replace_executor(ticket, replacement(1, false), &world)
        .unwrap();
    assert_eq!(new_ticket.revision, ticket.revision + 1);
    assert_eq!(
        movement.replace_executor(ticket, replacement(1, true), &world),
        Err(NavigationError::StaleTicket)
    );
    let mut game = Session::register(Workshop { movement }, &world).unwrap();
    for _ in 0..20 {
        game.step(&mut world, &[], 1. / 60.).unwrap();
    }
    let feedback = game.movement.actor(1).unwrap();
    assert_eq!(feedback.status, NavigationStatus::Unreachable);
    assert_eq!(feedback.execution_ticket, new_ticket);
    let mut movement = game.movement.clone();
    movement
        .replace_executor(new_ticket, replacement(1, true), &world)
        .unwrap();
    let mut game = Session::register(Workshop { movement }, &world).unwrap();
    for _ in 0..240 {
        game.step(&mut world, &[], 1. / 60.).unwrap();
    }
    assert_eq!(
        game.movement.actor(1).unwrap().status,
        NavigationStatus::Arrived
    );
}

#[test]
fn world_changes_cancel_held_execution_before_physics_integration() {
    let (mut w, mut g) = fixture(true, false);
    start(&mut w, &mut g);
    for _ in 0..20 {
        g.step(&mut w, &[], 1. / 60.).unwrap();
    }
    let before = w.item(1).unwrap().transform.anchor;
    assert!(before.z > 1.);
    let mut items = w.items().to_vec();
    let mut obstacle = Item {
        id: 2,
        physics_body: Some(PhysicsBody::new(BodyKind::Static)),
        collider: Some(Collider::new(ColliderShape::Box {
            half_extents: Vec3::new(1., 1., 0.05),
        })),
        ..Default::default()
    };
    obstacle.transform.anchor = before + Vec3::new(0., 0., 0.4);
    items.push(obstacle);
    w = World::new(w.space().clone(), items);
    g.step(&mut w, &[], 1. / 60.).unwrap();
    assert_eq!(w.item(1).unwrap().transform.anchor, before);
    assert_ne!(
        g.movement.actor(1).unwrap().status,
        NavigationStatus::Arrived
    );
}

#[test]
fn executor_failure_is_reported_and_does_not_leave_motion_held() {
    let (mut w, g) = fixture(true, false);
    let mut movement = g.movement.clone();
    let ticket = movement
        .request(Envelope::new(
            MessageId(1),
            NavigationRequest::Start {
                actor: 1,
                goal: NavigationGoal::Position(Vec3::new(0., 0., 3.)),
            },
        ))
        .payload
        .unwrap();
    let executor = Lift {
        id: 1,
        capability: Capability {
            item: 1,
            vertical: true,
            invalid_cost: false,
        },
        step: None,
        elapsed: 0.,
        calibrations: 0,
        blocked: false,
        fail_execution: true,
    };
    movement.replace_executor(ticket, executor, &w).unwrap();
    let mut game = Session::register(Workshop { movement }, &w).unwrap();
    for _ in 0..60 {
        game.step(&mut w, &[], 1. / 60.).unwrap();
    }
    assert_eq!(
        game.movement.actor(1).unwrap().status,
        NavigationStatus::Failed
    );
    assert_eq!(w.item(1).unwrap().transform.anchor.z, 1.);
}
