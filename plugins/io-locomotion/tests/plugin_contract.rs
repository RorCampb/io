//! An external developer's plugin: no dependency on the playground or its enemy rules.
use io_game::{GamePlugin, PluginInfo, PluginWorld, Session, Tick};
use io_locomotion::*;
use io_types::{Envelope, MessageId, Vec3};
use io_world::{
    AnimationState, BodyKind, CharacterBody, Collider, ColliderShape, Item, PhysicsBody, Space,
    Transform, World, WorldView,
};

fn fixture() -> (World, Movement) {
    fixture_with_obstacles(vec![])
}
fn fixture_with_obstacles(obstacles: Vec<Item>) -> (World, Movement) {
    let hero = Item {
        id: 1,
        renderable: Some(Default::default()),
        animation: Some(AnimationState::looping(0)),
        character_body: Some(CharacterBody {
            radius: 0.35,
            height: 1.9,
            max_slope: 0.8,
        }),
        transform: Transform::new(Vec3::default(), Vec3::new(1., 1., 1.), 0.).unwrap(),
        ..Default::default()
    };
    let floor = Item {
        id: 2,
        collider: Some(Collider::new(ColliderShape::Box {
            half_extents: Vec3::new(10., 10., 0.5),
        })),
        physics_body: Some(PhysicsBody::new(BodyKind::Static)),
        transform: Transform::new(Vec3::new(0., 0., -0.5), Vec3::new(20., 20., 1.), 0.).unwrap(),
        ..Default::default()
    };
    let mut items = vec![hero, floor];
    items.extend(obstacles);
    let world = World::new(Space::new(Vec3::new(20., 20., 10.)), items);
    let settings = Locomotion {
        walk_speed: 2.1,
        crouch_speed: 0.95,
        jump_speed: 4.5,
        gravity: 9.81,
        standing_height: 1.9,
        crouch_height: 1.15,
        turn_speed: 8.,
        blend_seconds: 0.12,
        clips: Motion::ALL
            .into_iter()
            .map(|m| {
                (
                    m,
                    ClipDefinition {
                        clip: format!("{m:?}"),
                        speed: 1.,
                        looping: true,
                        fixed_root_height: false,
                    },
                )
            })
            .collect(),
    };
    let clips = Motion::ALL
        .into_iter()
        .enumerate()
        .map(|(i, m)| (m, (i, 1.)))
        .collect();
    let motor = Motor::new(settings, 1, clips, &world).unwrap();
    let movement = Movement::new(
        io_traversal::navigation::Domain {
            origin: [-9., -9., 0.],
            size: [18., 18.],
            cell_size: 0.5,
        },
        32,
        vec![ActorBinding {
            steering: SteeringSettings::default(),
            motor,
            can_crouch: true,
            familiar_points: vec![],
        }],
        &world,
    )
    .unwrap();
    (world, movement)
}

#[derive(Clone, Copy, Debug)]
enum Reaction {
    Wait,
    ReturnHome,
}
#[derive(Clone, Debug)]
enum MyEvent {
    Movement(MovementEvent),
    TaskFinished,
    Noise,
}
#[derive(Clone, Debug)]
struct MyGame {
    movement: Movement,
    reaction: Reaction,
    reacted: bool,
    next: u64,
    reply: Option<Envelope<Result<NavigationTicket, NavigationError>>>,
}
impl MyGame {
    fn request(&mut self, request: NavigationRequest) {
        self.next += 1;
        self.reply = Some(
            self.movement
                .request(Envelope::new(MessageId(self.next), request)),
        );
    }
}
impl GamePlugin for MyGame {
    type Command = NavigationRequest;
    type Event = MyEvent;
    type Error = Error;
    fn info(&self) -> PluginInfo {
        PluginInfo {
            id: "third-party-example",
            version: 1,
        }
    }
    fn validate(&self, w: &dyn WorldView) -> Result<(), Error> {
        self.movement.validate(w)
    }
    fn active_items(&self) -> Vec<u64> {
        self.movement.active_items()
    }
    fn command(&mut self, _: &mut PluginWorld<MyEvent>, c: NavigationRequest) -> Result<(), Error> {
        self.request(c);
        Ok(())
    }
    fn update(&mut self, w: &mut PluginWorld<MyEvent>, tick: Tick) -> Result<(), Error> {
        let frame = self.movement.update(w, tick.seconds())?;
        for f in &frame.actors {
            assert_eq!(f.ticket.actor, f.execution_ticket.actor);
            assert!(f.ticket.revision >= f.execution_ticket.revision);
            assert_eq!(f.tick, frame.tick);
        }
        for event in frame.events {
            w.emit(MyEvent::Movement(event));
        }
        let actor = frame.actors[0];
        // Essential rules consume authoritative feedback, not the bounded event history.
        if actor.status == NavigationStatus::Arrived && !self.reacted {
            self.reacted = true;
            let request = match self.reaction {
                Reaction::Wait => NavigationRequest::Cancel {
                    ticket: actor.ticket,
                },
                Reaction::ReturnHome => NavigationRequest::Replace {
                    ticket: actor.ticket,
                    goal: NavigationGoal::Position(Vec3::default()),
                },
            };
            self.request(request);
            assert!(self.reply.as_ref().unwrap().payload.is_ok());
            w.emit(MyEvent::TaskFinished);
            for _ in 0..300 {
                w.emit(MyEvent::Noise);
            }
        }
        Ok(())
    }
}
fn game(reaction: Reaction) -> (World, Session<MyGame>) {
    let (world, movement) = fixture();
    game_from(world, movement, reaction)
}
fn game_from(world: World, movement: Movement, reaction: Reaction) -> (World, Session<MyGame>) {
    let plugin = MyGame {
        movement,
        reaction,
        reacted: false,
        next: 0,
        reply: None,
    };
    let session = Session::register(plugin, &world).unwrap();
    (world, session)
}

#[derive(Debug)]
struct SlowSteering;
impl Steering for SlowSteering {
    fn steer(&self, input: SteeringInput) -> Result<SteeringOutput, Error> {
        let delta = input.target - input.position;
        let length = delta.x.hypot(delta.y);
        let speed = 0.5_f32.min(input.max_speed).min(length / input.seconds);
        SteeringOutput::new(Vec3::new(delta.x, delta.y, 0.).scaled(speed / length.max(0.0001)))
    }
}

#[derive(Debug)]
struct ConstantSteering(Vec3);
impl Steering for ConstantSteering {
    fn steer(&self, _: SteeringInput) -> Result<SteeringOutput, Error> {
        SteeringOutput::new(self.0)
    }
}

fn start(world: &mut World, game: &mut Session<MyGame>, goal: Vec3) {
    game.command(
        world,
        NavigationRequest::Start {
            actor: 1,
            goal: NavigationGoal::Position(goal),
        },
    )
    .unwrap();
}

fn remote_box() -> Item {
    Item {
        id: 9,
        physics_body: Some(PhysicsBody::new(BodyKind::Kinematic)),
        collider: Some(Collider::new(ColliderShape::Box {
            half_extents: Vec3::new(0.5, 0.5, 0.5),
        })),
        transform: Transform::new(Vec3::new(-7., -7., 0.5), Vec3::new(1., 1., 1.), 0.).unwrap(),
        ..Default::default()
    }
}

#[test]
fn unrelated_geometry_churn_does_not_cancel_routes_or_background_work() {
    let (world, movement) = fixture_with_obstacles(vec![remote_box()]);
    let (mut world, mut game) = game_from(
        world,
        movement.with_planning(io_traversal::PlanningMode::Background),
        Reaction::Wait,
    );
    start(&mut world, &mut game, Vec3::new(6., 0., 0.));
    for tick in 0..600 {
        assert!(world.set_kinematic_target(
            9,
            Vec3::new(-7., -7. + (tick % 2) as f32, 0.5),
            Default::default()
        ));
        game.step(&mut world, &[], 1. / 60.).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1));
        if game.reacted {
            break;
        }
    }
    assert!(
        game.reacted,
        "revision churn prevented arrival: {:?}",
        game.movement.actor(1)
    );
    assert!(world.item(1).unwrap().transform.anchor.x > 5.9);
    let stats = game.movement.planning_stats();
    assert_eq!(
        stats.cancelled, 0,
        "unrelated changes cancelled work: {stats:?}"
    );
    assert_eq!(
        stats.accepted, 1,
        "unrelated changes requested replacement: {stats:?}"
    );
}

#[test]
fn reconsider_is_ticket_checked_coalesces_and_keeps_executing_in_both_modes() {
    for mode in [
        io_traversal::PlanningMode::Inline,
        io_traversal::PlanningMode::Background,
    ] {
        let (world, movement) = fixture();
        let (mut world, mut game) = game_from(world, movement.with_planning(mode), Reaction::Wait);
        start(&mut world, &mut game, Vec3::new(8., 0., 0.));
        for _ in 0..400 {
            game.step(&mut world, &[], 1. / 60.).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(1));
            if world.item(1).unwrap().transform.anchor.x > 1. {
                break;
            }
        }
        let ticket = game.movement.actor(1).unwrap().ticket;
        let start = world.item(1).unwrap().transform.anchor;
        for _ in 0..60 {
            game.command(&mut world, NavigationRequest::Reconsider { ticket })
                .unwrap();
            assert_eq!(game.reply.as_ref().unwrap().payload, Ok(ticket));
            game.step(&mut world, &[], 1. / 60.).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(
            world.item(1).unwrap().transform.anchor.x > start.x + 1.,
            "{mode:?}"
        );
        assert_eq!(game.movement.planning_stats().cancelled, 0);
        let stale = NavigationTicket {
            revision: ticket.revision - 1,
            ..ticket
        };
        game.command(&mut world, NavigationRequest::Reconsider { ticket: stale })
            .unwrap();
        assert_eq!(
            game.reply.as_ref().unwrap().payload,
            Err(NavigationError::StaleTicket)
        );
    }
}

#[test]
fn notice_flood_does_not_restart_an_incomplete_inline_search() {
    let (world, original) = fixture();
    let movement = Movement::new(
        io_traversal::navigation::Domain {
            origin: [-9., -9., 0.],
            size: [18., 18.],
            cell_size: 0.5,
        },
        1,
        vec![io_traversal::ActorBinding {
            executor: original.executor(1).unwrap().clone(),
            familiar_points: vec![],
            look_ahead: 3.,
        }],
        &world,
    )
    .unwrap();
    let (mut world, mut game) = game_from(world, movement, Reaction::Wait);
    start(&mut world, &mut game, Vec3::new(8., 0., 0.));
    let ticket = game.movement.actor(1).unwrap().ticket;
    for _ in 0..120 {
        game.command(&mut world, NavigationRequest::Reconsider { ticket })
            .unwrap();
        game.step(&mut world, &[], 1. / 60.).unwrap();
    }
    assert!(world.item(1).unwrap().transform.anchor.x > 1.);
    assert!(
        game.movement.stats().expanded <= 120,
        "shared budget must still hold"
    );
}

#[test]
fn occupied_goal_allows_approach_wait_and_resume_without_a_new_route() {
    let occupant = Item {
        id: 8,
        character_body: Some(CharacterBody {
            radius: 0.35,
            height: 1.9,
            max_slope: 0.8,
        }),
        transform: Transform::new(Vec3::new(4., 0., 0.), Vec3::new(1., 1., 1.), 0.).unwrap(),
        ..Default::default()
    };
    let (world, movement) = fixture_with_obstacles(vec![occupant]);
    let (mut world, mut game) = game_from(world, movement, Reaction::Wait);
    start(&mut world, &mut game, Vec3::new(4., 0., 0.));
    for _ in 0..300 {
        game.step(&mut world, &[], 1. / 60.).unwrap();
    }
    let p = world.item(1).unwrap().transform.anchor;
    assert!(
        p.x > 2.5 && (p - Vec3::new(4., 0., 0.)).dot(p - Vec3::new(4., 0., 0.)) >= 0.69 * 0.69,
        "{p:?}"
    );
    assert_eq!(
        game.movement.actor(1).unwrap().status,
        NavigationStatus::Following
    );
    let expanded = game.movement.stats().expanded;
    world.set_pose(8, Vec3::new(4., 3., 0.), 0.);
    for _ in 0..180 {
        game.step(&mut world, &[], 1. / 60.).unwrap();
    }
    assert!(game.reacted);
    assert_eq!(game.movement.stats().expanded, expanded);
}

#[test]
fn unobserved_wall_still_blocks_an_accepted_route() {
    let (world, movement) = fixture_with_obstacles(vec![remote_box()]);
    let (mut world, mut game) = game_from(world, movement, Reaction::Wait);
    start(&mut world, &mut game, Vec3::new(6., 0., 0.));
    for _ in 0..45 {
        game.step(&mut world, &[], 1. / 60.).unwrap();
    }
    assert!(world.set_kinematic_target(9, Vec3::new(3., 0., 0.5), Default::default()));
    // Unobserved changes are now caught at immediate execution distance, not
    // by a repeated three-metre shortcut scan. Allow the later encounter/retry.
    for _ in 0..480 {
        game.step(&mut world, &[], 1. / 60.).unwrap();
        let p = world.item(1).unwrap().transform.anchor;
        assert!(
            p.x <= 2.151 || p.x >= 3.849 || p.y.abs() >= 0.849,
            "penetrated unobserved wall: {p:?}"
        );
    }
    assert!(
        game.reacted,
        "should safely route around the wall: {:?}, route {:?}",
        game.movement.actor(1),
        game.movement.route(1)
    );
}

#[test]
fn external_plugin_replaces_steering_without_replacing_navigation_or_motor() {
    let (world, mut movement) = fixture();
    assert_eq!(
        movement.set_steering(99, SlowSteering, &world),
        Err(NavigationError::UnknownActor)
    );
    movement.set_steering(1, SlowSteering, &world).unwrap();
    let (mut world, mut game) = game_from(world, movement, Reaction::Wait);
    start(&mut world, &mut game, Vec3::new(4., 0., 0.));
    for _ in 0..120 {
        game.step(&mut world, &[], 1. / 60.).unwrap();
    }
    let feedback = game.movement.actor(1).unwrap();
    assert!((feedback.execution.position.x - 1.).abs() < 0.05);
    assert!((feedback.execution.actual_velocity.x - 0.5).abs() < 0.001);
    assert_eq!(feedback.status, NavigationStatus::Following);
}

#[test]
fn custom_steering_cannot_submit_nan_vertical_motion_or_excess_speed() {
    for velocity in [
        Vec3::new(f32::NAN, 0., 0.),
        Vec3::new(0., 0., 1.),
        Vec3::new(30., 0., 0.),
    ] {
        let (world, mut movement) = fixture();
        movement
            .set_steering(1, ConstantSteering(velocity), &world)
            .unwrap();
        let (mut world, mut game) = game_from(world, movement, Reaction::Wait);
        start(&mut world, &mut game, Vec3::new(4., 0., 0.));
        let mut rejected = false;
        for _ in 0..60 {
            if game.step(&mut world, &[], 1. / 60.).is_err() {
                rejected = true;
                break;
            }
        }
        assert!(rejected);
        assert_eq!(world.item(1).unwrap().transform.anchor, Vec3::default());
    }
}

fn wall(position: Vec3, half_extents: Vec3) -> Item {
    Item {
        id: 3,
        transform: Transform::new(position, Vec3::new(1., 1., 1.), 0.).unwrap(),
        collider: Some(Collider::new(ColliderShape::Box { half_extents })),
        physics_body: Some(PhysicsBody::new(BodyKind::Static)),
        ..Default::default()
    }
}

#[test]
fn steering_shortcuts_diagonal_route_and_arrives_without_orbiting_at_multiple_rates() {
    for hz in [30, 60, 144] {
        let (mut world, mut game) = game(Reaction::Wait);
        let goal = Vec3::new(5., 4., 0.);
        start(&mut world, &mut game, goal);
        let mut last = Vec3::default();
        let mut length = 0.;
        let mut diagonal_ticks = 0;
        for _ in 0..hz * 8 {
            let before = game.movement.stats().steering_queries;
            game.step(&mut world, &[], 1. / hz as f32).unwrap();
            assert!(game.movement.stats().steering_queries - before <= 9);
            let p = world.item(1).unwrap().transform.anchor;
            let delta = p - last;
            length += delta.dot(delta).sqrt();
            diagonal_ticks += usize::from(delta.x > 0.001 && delta.y > 0.001);
            last = p;
        }
        assert!(game.reacted, "failed to arrive at {hz} Hz: {last:?}");
        assert!((last - goal).dot(last - goal).sqrt() < 0.02);
        assert!(
            length < goal.dot(goal).sqrt() * 1.06,
            "zigzag distance at {hz} Hz: {length}"
        );
        assert!(diagonal_ticks > hz as usize);
        println!(
            "{hz} Hz: path {length:.3} m vs direct {:.3} m",
            goal.dot(goal).sqrt()
        );
    }
}

#[test]
fn prepared_straight_route_does_not_brake_at_every_grid_or_refinement_point() {
    for hz in [30, 60, 144] {
        let (mut world, mut game) = game(Reaction::Wait);
        start(&mut world, &mut game, Vec3::new(8., 0., 0.));
        let mut slow_mid_route = 0;
        let mut middle_ticks = 0;
        for _ in 0..hz * 6 {
            game.step(&mut world, &[], 1. / hz as f32).unwrap();
            let f = game.movement.actor(1).unwrap().execution;
            if (2.5..6.5).contains(&f.position.x) {
                middle_ticks += 1;
                slow_mid_route += usize::from(f.actual_velocity.x < 1.8);
            }
        }
        assert!(game.reacted, "failed to finish at {hz} Hz");
        assert!(middle_ticks > 0);
        assert!(
            slow_mid_route * 20 < middle_ticks,
            "unnecessary stops at {hz} Hz: {slow_mid_route}/{middle_ticks}"
        );
        assert!(game.movement.stats().refinement_queries > 0);
    }
}

#[test]
fn rolling_curves_execute_a_body_clear_detour_at_multiple_tick_rates() {
    use io_locomotion::trajectory::{CornerRefiner, TrajectorySettings};
    for hz in [30, 60, 144] {
        let obstacle = wall(Vec3::new(2.5, 0., 1.5), Vec3::new(0.5, 1.5, 1.5));
        let (mut world, mut movement) = fixture_with_obstacles(vec![obstacle]);
        movement
            .set_trajectory_refiner(1, TrajectorySettings::default(), CornerRefiner, &world)
            .unwrap();
        let (w, mut game) = game_from(world, movement, Reaction::Wait);
        world = w;
        start(&mut world, &mut game, Vec3::new(5., 0., 0.));
        let mut turning_motion = 0;
        let mut peak_change = 0_f32;
        let mut previous = Vec3::default();
        for _ in 0..hz * 16 {
            let checks = game.movement.executor(1).unwrap().trajectory_stats().checks;
            game.step(&mut world, &[], 1. / hz as f32).unwrap();
            assert!(
                game.movement.executor(1).unwrap().trajectory_stats().checks - checks <= 2,
                "local preparation/preview budget exceeded"
            );
            let f = game.movement.actor(1).unwrap().execution;
            assert!(io_world::character_fits(
                &world,
                1,
                f.position,
                world.item(1).unwrap().character_body.unwrap()
            ));
            if !game
                .movement
                .executor(1)
                .unwrap()
                .local_trajectory()
                .is_empty()
            {
                turning_motion += 1;
                peak_change = peak_change.max(
                    (f.actual_velocity - previous)
                        .dot(f.actual_velocity - previous)
                        .sqrt(),
                );
            }
            previous = f.actual_velocity;
        }
        let stats = game.movement.executor(1).unwrap().trajectory_stats();
        assert!(
            game.reacted,
            "{hz} Hz failed: {:?}, {stats:?}",
            game.movement.actor(1)
        );
        assert!(
            stats.accepted > 0 && turning_motion > 0,
            "{hz} Hz never followed curve: {stats:?}"
        );
        assert!(
            peak_change < 14. / hz as f32 + 0.03,
            "{hz}Hz velocity discontinuity {peak_change}"
        );
        println!("{hz}Hz: {stats:?}, peak dv={peak_change}");
    }
}

#[test]
fn local_preview_requests_replacement_before_contact_and_retains_ticket() {
    use io_locomotion::trajectory::{CornerRefiner, TrajectorySettings};
    let (world, mut movement) = fixture_with_obstacles(vec![remote_box()]);
    movement
        .set_trajectory_refiner(1, TrajectorySettings::default(), CornerRefiner, &world)
        .unwrap();
    let (mut world, mut game) = game_from(world, movement, Reaction::Wait);
    start(&mut world, &mut game, Vec3::new(6., 0., 0.));
    for _ in 0..35 {
        game.step(&mut world, &[], 1. / 60.).unwrap();
    }
    let ticket = game.movement.actor(1).unwrap().ticket;
    assert!(world.set_kinematic_target(9, Vec3::new(3., 0., 0.5), Default::default()));
    let mut early = false;
    for _ in 0..600 {
        game.step(&mut world, &[], 1. / 60.).unwrap();
        let f = game.movement.actor(1).unwrap();
        if !early
            && game
                .movement
                .executor(1)
                .unwrap()
                .trajectory_stats()
                .anticipations
                > 0
        {
            assert!(
                f.execution.position.x < 1.6,
                "late preview: {:?}",
                f.execution
            );
            assert_eq!(f.ticket, ticket);
            early = true;
        }
        assert!(io_world::character_fits(
            &world,
            1,
            f.execution.position,
            world.item(1).unwrap().character_body.unwrap()
        ));
    }
    assert!(
        early && game.reacted,
        "preview/detour failed: {:?}",
        game.movement.actor(1)
    );
}

#[test]
fn invalid_external_curve_is_rejected_and_replacing_strategy_preserves_contracts() {
    use io_locomotion::trajectory::*;
    #[derive(Debug)]
    struct InvalidCurve;
    impl TrajectoryRefiner for InvalidCurve {
        fn propose(&self, _: RefinementInput) -> Option<CurveProposal> {
            Some(CurveProposal {
                controls: [Vec3::new(f32::NAN, 0., 0.); 4],
            })
        }
    }
    let obstacle = wall(Vec3::new(2.5, 0., 1.5), Vec3::new(0.5, 1.5, 1.5));
    let (world, mut movement) = fixture_with_obstacles(vec![obstacle]);
    let original = movement.actor(1).unwrap().ticket;
    assert!(movement
        .set_trajectory_refiner(
            1,
            TrajectorySettings {
                samples: 1000,
                ..Default::default()
            },
            InvalidCurve,
            &world
        )
        .is_err());
    assert_eq!(movement.actor(1).unwrap().ticket, original);
    let ticket = movement
        .set_trajectory_refiner(1, TrajectorySettings::default(), InvalidCurve, &world)
        .unwrap();
    assert_ne!(ticket, original);
    let (mut world, mut game) = game_from(world, movement, Reaction::Wait);
    start(&mut world, &mut game, Vec3::new(5., 0., 0.));
    let mut rejected = false;
    for _ in 0..300 {
        if game.step(&mut world, &[], 1. / 60.).is_err() {
            rejected = true;
            break;
        }
    }
    assert!(rejected);
    assert!(world.item(1).unwrap().transform.anchor.finite());
}

#[test]
fn rounded_detour_keeps_body_clear_and_custom_steering_cannot_cut_through_wall() {
    let obstacle = wall(Vec3::new(2.5, 0., 1.5), Vec3::new(0.5, 1.5, 1.5));
    let (world, movement) = fixture_with_obstacles(vec![obstacle.clone()]);
    let (mut world, mut game) = game_from(world, movement, Reaction::Wait);
    start(&mut world, &mut game, Vec3::new(5., 0., 0.));
    let mut detour = 0_f32;
    for _ in 0..900 {
        game.step(&mut world, &[], 1. / 60.).unwrap();
        let actor = world.item(1).unwrap();
        assert!(io_world::character_fits(
            &world,
            1,
            actor.transform.anchor,
            actor.character_body.unwrap()
        ));
        detour = detour.max(actor.transform.anchor.y.abs());
    }
    assert!(game.reacted, "detour failed: {:?}", game.movement.actor(1));
    assert!(detour > 1.85);

    let (world, mut movement) = fixture_with_obstacles(vec![obstacle]);
    movement
        .set_steering(1, ConstantSteering(Vec3::new(2.1, 0., 0.)), &world)
        .unwrap();
    let (mut world, mut game) = game_from(world, movement, Reaction::Wait);
    start(&mut world, &mut game, Vec3::new(5., 0., 0.));
    for _ in 0..300 {
        game.step(&mut world, &[], 1. / 60.).unwrap();
        assert!(world.item(1).unwrap().transform.anchor.x < 1.651);
    }
}

#[test]
fn replacing_destination_brakes_old_direction_instead_of_finishing_old_route() {
    let (mut world, mut game) = game(Reaction::Wait);
    start(&mut world, &mut game, Vec3::new(8., 0., 0.));
    for _ in 0..60 {
        game.step(&mut world, &[], 1. / 60.).unwrap();
    }
    let before = game.movement.actor(1).unwrap();
    game.command(
        &mut world,
        NavigationRequest::Replace {
            ticket: before.ticket,
            goal: NavigationGoal::Position(Vec3::new(-4., 0., 0.)),
        },
    )
    .unwrap();
    let mut furthest = before.execution.position.x;
    for _ in 0..180 {
        game.step(&mut world, &[], 1. / 60.).unwrap();
        furthest = furthest.max(world.item(1).unwrap().transform.anchor.x);
    }
    assert!(furthest - before.execution.position.x < 0.3);
    assert!(world.item(1).unwrap().transform.anchor.x < before.execution.position.x - 1.);
}

#[test]
fn leased_urgency_brakes_without_losing_route_and_expires_at_all_tick_rates() {
    use io_traversal::NavigationPriority;
    for hz in [30, 60, 144] {
        let dt = 1. / hz as f32;
        let (mut world, mut game) = game(Reaction::Wait);
        start(&mut world, &mut game, Vec3::new(8., 0., 0.));
        for _ in 0..hz {
            game.step(&mut world, &[], dt).unwrap();
        }
        let before = game.movement.actor(1).unwrap();
        assert!(before.execution.actual_velocity.x > 1.);
        let hint = NavigationRequest::Prioritize {
            ticket: before.ticket,
            priority: NavigationPriority::Urgent,
            seconds: 0.5,
        };
        game.command(&mut world, hint).unwrap();
        for _ in 0..hz / 3 {
            game.step(&mut world, &[], dt).unwrap();
        }
        let stopped = game.movement.actor(1).unwrap();
        assert_eq!(stopped.ticket, before.ticket);
        assert!(
            stopped.execution.actual_velocity.x.abs() < 0.05,
            "{stopped:?}"
        );
        assert!(game.movement.route(1).is_some_and(|r| !r.is_empty()));
        for _ in 0..hz {
            game.step(&mut world, &[], dt).unwrap();
        }
        assert!(game.movement.actor(1).unwrap().execution.actual_velocity.x > 1.);
        let invalid = game.movement.clone().request(Envelope::new(
            MessageId(99),
            NavigationRequest::Prioritize {
                ticket: before.ticket,
                priority: NavigationPriority::Urgent,
                seconds: f32::NAN,
            },
        ));
        assert_eq!(invalid.payload, Err(NavigationError::InvalidConfiguration));
    }
}

#[test]
fn envelopes_correlate_and_stale_or_invalid_requests_cannot_replace_the_current_goal() {
    let (_, mut m) = fixture();
    let goal = NavigationGoal::Position(Vec3::new(3., 0., 0.));
    let reply = m.request(Envelope::new(
        MessageId(10),
        NavigationRequest::Start { actor: 1, goal },
    ));
    assert_eq!(reply.id, MessageId(10));
    let first = reply.payload.unwrap();
    assert_eq!(m.actor(1).unwrap().status, NavigationStatus::Planning);
    assert_ne!(first, m.actor(1).unwrap().execution_ticket);
    let second = m
        .request(Envelope::new(
            MessageId(11),
            NavigationRequest::Replace {
                ticket: first,
                goal,
            },
        ))
        .payload
        .unwrap();
    assert!(second.revision > first.revision);
    assert_eq!(
        m.request(Envelope::new(
            MessageId(12),
            NavigationRequest::Cancel { ticket: first }
        ))
        .payload,
        Err(NavigationError::StaleTicket)
    );
    assert_eq!(
        m.request(Envelope::new(
            MessageId(13),
            NavigationRequest::Replace {
                ticket: second,
                goal: NavigationGoal::Position(Vec3::new(f32::NAN, 0., 0.))
            }
        ))
        .payload,
        Err(NavigationError::InvalidGoal)
    );
    assert_eq!(m.actor(1).unwrap().ticket, second);
    assert_eq!(
        m.request(Envelope::new(
            MessageId(14),
            NavigationRequest::Start { actor: 99, goal }
        ))
        .payload,
        Err(NavigationError::UnknownActor)
    );
}

#[test]
fn the_same_arrived_fact_supports_two_plugin_reactions_even_if_history_overflows() {
    for reaction in [Reaction::Wait, Reaction::ReturnHome] {
        let (mut world, mut game) = game(reaction);
        game.command(
            &mut world,
            NavigationRequest::Start {
                actor: 1,
                goal: NavigationGoal::Position(Vec3::new(3., 0., 0.)),
            },
        )
        .unwrap();
        for _ in 0..420 {
            game.step(&mut world, &[], 1. / 60.).unwrap();
        }
        assert!(game.reacted && game.dropped_events() > 0);
        let x = world.item(1).unwrap().transform.anchor.x;
        match reaction {
            Reaction::Wait => assert!((x - 3.).abs() < 0.02),
            Reaction::ReturnHome => assert!(x.abs() < 0.02),
        }
        assert_eq!(world.item(1).unwrap().simulated_ticks, 420);
        for event in game.events() {
            if let MyEvent::Movement(MovementEvent::Navigation { ticket, .. }) = &event.payload {
                assert_eq!(ticket.actor, 1);
            }
        }
    }
}

#[test]
fn cancel_stops_held_guidance_and_old_feedback_is_not_relabelled_as_new_execution() {
    let (mut world, mut game) = game(Reaction::Wait);
    game.command(
        &mut world,
        NavigationRequest::Start {
            actor: 1,
            goal: NavigationGoal::Position(Vec3::new(8., 0., 0.)),
        },
    )
    .unwrap();
    for _ in 0..60 {
        game.step(&mut world, &[], 1. / 60.).unwrap();
    }
    let before = game.movement.actor(1).unwrap();
    assert!(before.execution.actual_velocity.x > 1.);
    game.command(
        &mut world,
        NavigationRequest::Cancel {
            ticket: before.ticket,
        },
    )
    .unwrap();
    let pending = game.movement.actor(1).unwrap();
    assert_eq!(pending.status, NavigationStatus::Idle);
    assert_eq!(pending.execution_ticket, before.ticket);
    assert_ne!(pending.ticket, pending.execution_ticket);
    for _ in 0..60 {
        game.step(&mut world, &[], 1. / 60.).unwrap();
    }
    let after = game.movement.actor(1).unwrap();
    assert!((after.execution.position.x - before.execution.position.x).abs() < 0.001);
    assert_eq!(after.execution.requested_velocity, Vec3::default());
    assert_eq!(after.execution.actual_velocity.x, 0.);
    assert_eq!(after.ticket, after.execution_ticket);
}
