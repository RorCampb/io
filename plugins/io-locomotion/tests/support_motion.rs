//! The real character motor in a developer plugin, without the grid planner.
use io_game::{GamePlugin, PluginInfo, PluginWorld, Session, Tick};
use io_locomotion::{
    ActorBinding, ClipDefinition, Command, Error, Locomotion, Motion, MotionEvent, Motor, Movement,
    NavigationGoal, NavigationRequest, NavigationStatus, SteeringSettings,
};
use io_types::{Rotation, Vec3};
use io_world::{
    character_fits, character_support, AnimationState, BodyKind, CharacterBody, Collider,
    ColliderShape, Item, PhysicsBody, Space, SupportProbe, Transform, World, WorldView,
};

#[derive(Clone, Debug)]
struct MotorDemo {
    motor: Motor,
    jumps: u32,
    landings: u32,
}
impl GamePlugin for MotorDemo {
    type Command = Command;
    type Event = ();
    type Error = Error;
    fn info(&self) -> PluginInfo {
        PluginInfo {
            id: "support-motor-test",
            version: 1,
        }
    }
    fn validate(&self, world: &dyn WorldView) -> Result<(), Error> {
        self.motor.validate(world)
    }
    fn active_items(&self) -> Vec<u64> {
        vec![1]
    }
    fn command(&mut self, _: &mut PluginWorld<()>, command: Command) -> Result<(), Error> {
        self.motor.command(command)
    }
    fn update(&mut self, world: &mut PluginWorld<()>, tick: Tick) -> Result<(), Error> {
        for event in self.motor.update(world, tick.seconds())? {
            match event {
                MotionEvent::Jumped => self.jumps += 1,
                MotionEvent::Landed => self.landings += 1,
                MotionEvent::Stance { .. } => {}
            }
        }
        Ok(())
    }
}

fn fixture() -> (World, Session<MotorDemo>) {
    let angle = 0.3_f32;
    let body = CharacterBody {
        radius: 0.3,
        height: 1.8,
        max_slope: 0.8,
    };
    let ramp = Item {
        id: 2,
        transform: Transform::oriented(
            Vec3::new(0., 0., 2.),
            Vec3::new(1., 1., 1.),
            Rotation::from_xyzw([0., (angle * 0.5).sin(), 0., (angle * 0.5).cos()]).unwrap(),
        )
        .unwrap(),
        collider: Some(Collider::new(ColliderShape::Box {
            half_extents: Vec3::new(5., 3., 0.25),
        })),
        physics_body: Some(PhysicsBody::new(BodyKind::Static)),
        ..Item::default()
    };
    let mut world = World::new(Space::new(Vec3::new(100., 100., 100.)), vec![ramp]);
    let start = character_support(
        &world,
        None,
        Vec3::new(-2., 0., 2.),
        body,
        SupportProbe::new(4., 4.).unwrap(),
    )
    .unwrap()
    .unwrap()
    .anchor;
    let actor = Item {
        id: 1,
        character_body: Some(body),
        renderable: Some(Default::default()),
        animation: Some(AnimationState::looping(0)),
        transform: Transform::new(start, Vec3::new(1., 1., 1.), 0.).unwrap(),
        ..Item::default()
    };
    let mut items = world.items().to_vec();
    items.push(actor);
    world = World::new(world.space().clone(), items);
    let settings = Locomotion {
        walk_speed: 2.1,
        crouch_speed: 0.95,
        jump_speed: 4.5,
        gravity: 9.81,
        standing_height: 1.8,
        crouch_height: 1.1,
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
    let game = Session::register(
        MotorDemo {
            motor,
            jumps: 0,
            landings: 0,
        },
        &world,
    )
    .unwrap();
    (world, game)
}

fn flat_motor() -> (World, Motor) {
    let (source, game) = fixture();
    let mut actor = source.item(1).unwrap().clone();
    actor.transform.anchor = Vec3::new(-1., 0., 0.);
    let floor = Item {
        id: 2,
        transform: Transform::new(Vec3::new(0., 0., -0.5), Vec3::new(1., 1., 1.), 0.).unwrap(),
        collider: Some(Collider::new(ColliderShape::Box {
            half_extents: Vec3::new(4., 3., 0.5),
        })),
        physics_body: Some(PhysicsBody::new(BodyKind::Static)),
        ..Default::default()
    };
    (
        World::new(source.space().clone(), vec![actor, floor]),
        game.motor.clone(),
    )
}
fn motor_session(motor: Motor, w: &World) -> Session<MotorDemo> {
    Session::register(
        MotorDemo {
            motor,
            jumps: 0,
            landings: 0,
        },
        w,
    )
    .unwrap()
}
fn abilities() -> io_locomotion::athletics::Settings {
    io_locomotion::athletics::Settings {
        step_height: 0.3,
        jump_distance: 3.,
        max_drop: 0.6,
    }
}

#[test]
fn input_and_planned_jump_share_motor_gravity_and_landing_at_all_rates() {
    use io_locomotion::athletics::Action;
    use io_traversal::navigation::Waypoint;
    for hz in [30, 60, 144] {
        let (mut a, mut input) = flat_motor();
        let (mut b, mut planned) = flat_motor();
        let position = b.item(1).unwrap().transform.anchor;
        assert_eq!(
            planned
                .begin_traversal(
                    &b,
                    abilities(),
                    Waypoint {
                        position,
                        action: Action::Jump
                    }
                )
                .unwrap(),
            io_traversal::Progress::Running
        );
        input.command(Command::Jump).unwrap();
        let mut input = motor_session(input, &a);
        let mut planned = motor_session(planned, &b);
        let (mut apex_a, mut apex_b) = (0_f32, 0_f32);
        for _ in 0..hz * 2 {
            input.step(&mut a, &[], 1. / hz as f32).unwrap();
            planned.step(&mut b, &[], 1. / hz as f32).unwrap();
            apex_a = apex_a.max(a.item(1).unwrap().transform.anchor.z);
            apex_b = apex_b.max(b.item(1).unwrap().transform.anchor.z);
        }
        assert!((apex_a - apex_b).abs() < 0.002, "{hz}: {apex_a}/{apex_b}");
        assert_eq!((input.jumps, input.landings), (1, 1));
        assert_eq!((planned.jumps, planned.landings), (1, 1));
        assert!(input.motor.grounded() && planned.motor.grounded());
    }
}

#[test]
fn planned_jump_rejects_missing_support_and_overhead_obstacles() {
    use io_locomotion::athletics::Action;
    use io_traversal::{navigation::Waypoint, Progress};
    let (w, mut motor) = flat_motor();
    assert_eq!(
        motor
            .begin_traversal(
                &w,
                abilities(),
                Waypoint {
                    position: Vec3::new(6., 0., 0.),
                    action: Action::Jump
                }
            )
            .unwrap(),
        Progress::Blocked
    );
    let mut ceiling = w.item(2).unwrap().clone();
    ceiling.id = 3;
    ceiling.transform.anchor = Vec3::new(0., 0., 2.7);
    let mut items = w.items().to_vec();
    items.push(ceiling);
    let blocked = World::new(w.space().clone(), items);
    assert_eq!(
        motor
            .begin_traversal(
                &blocked,
                abilities(),
                Waypoint {
                    position: Vec3::new(0.5, 0., 0.),
                    action: Action::Jump
                }
            )
            .unwrap(),
        Progress::Blocked
    );
}

#[test]
fn cancelling_planned_jump_stops_guidance_but_gravity_still_lands() {
    use io_locomotion::athletics::Action;
    use io_traversal::navigation::Waypoint;
    let (mut w, mut motor) = flat_motor();
    motor
        .begin_traversal(
            &w,
            abilities(),
            Waypoint {
                position: Vec3::new(0.5, 0., 0.),
                action: Action::Jump,
            },
        )
        .unwrap();
    let mut game = motor_session(motor, &w);
    for _ in 0..18 {
        game.step(&mut w, &[], 1. / 60.).unwrap();
    }
    let stopped = w.item(1).unwrap().transform.anchor;
    assert!(stopped.z > 0.5);
    let mut motor = game.motor.clone();
    motor.cancel();
    let mut game = motor_session(motor, &w);
    for _ in 0..120 {
        game.step(&mut w, &[], 1. / 60.).unwrap();
    }
    let end = w.item(1).unwrap().transform.anchor;
    assert!((end.x - stopped.x).abs() < 1e-5);
    assert!(end.z.abs() < 0.003 && game.motor.grounded());
}

#[test]
fn real_motor_follows_ramps_jumps_and_lands_at_30_60_and_144_hz() {
    for hz in [30, 60, 144] {
        let (mut world, mut game) = fixture();
        let initial = world.item(1).unwrap().transform.anchor;
        let dt = 1. / hz as f32;
        game.command(&mut world, Command::Move { x: 1., y: 0. })
            .unwrap();
        for _ in 0..hz * 2 {
            game.step(&mut world, &[], dt).unwrap();
            let actor = world.item(1).unwrap();
            assert!(character_fits(
                &world,
                1,
                actor.transform.anchor,
                actor.character_body.unwrap()
            ));
            assert!(
                game.motor.grounded(),
                "hz={hz} {:?}",
                actor.transform.anchor
            );
        }
        let walked = world.item(1).unwrap().transform.anchor;
        assert!((walked.x - initial.x - 4.2).abs() < 0.001);
        assert!((walked.z - initial.z + 4.2 * 0.3_f32.tan()).abs() < 0.001);
        game.command(&mut world, Command::Move { x: 0., y: 0. })
            .unwrap();
        game.command(&mut world, Command::Jump).unwrap();
        game.step(&mut world, &[], dt).unwrap();
        assert!(!game.motor.grounded());
        assert_eq!(game.jumps, 1);
        // Holding/repeating jump in air cannot manufacture support or a second jump.
        game.command(&mut world, Command::Jump).unwrap();
        for _ in 0..hz * 2 {
            game.step(&mut world, &[], dt).unwrap();
        }
        assert_eq!(game.jumps, 1);
        assert_eq!(game.landings, 1);
        assert!(game.motor.grounded());
        assert!((world.item(1).unwrap().transform.anchor.z - walked.z).abs() < 0.001);
    }
}

#[test]
fn player_can_leave_a_ledge_instead_of_being_glued_to_the_last_floor() {
    let (mut world, mut game) = fixture();
    game.command(&mut world, Command::Move { x: 0., y: 1. })
        .unwrap();
    for _ in 0..120 {
        game.step(&mut world, &[], 1. / 60.).unwrap();
    }
    assert!(world.item(1).unwrap().transform.anchor.y > 4.);
    assert!(!game.motor.grounded());
    assert!(game.motor.vertical_speed() < 0.);
}

#[test]
fn real_motor_crosses_a_terrain_crest_without_losing_support() {
    for hz in [30, 60, 144] {
        let (old, mut game) = fixture();
        let actor = old.item(1).unwrap().clone();
        let body = actor.character_body.unwrap();
        let mut world = World::new(old.space().clone(), vec![actor]);
        world.set_terrain(
            io_world::HeightField::new(
                [0., -2.],
                2.,
                3,
                3,
                vec![0., 1., 0., 0., 1., 0., 0., 1., 0.],
            )
            .unwrap(),
        );
        let start = character_support(
            &world,
            Some(1),
            Vec3::new(0.4, 0., 0.),
            body,
            SupportProbe::new(2., 2.).unwrap(),
        )
        .unwrap()
        .unwrap()
        .anchor;
        assert!(world.set_pose(1, start, 0.));
        game.command(&mut world, Command::Move { x: 1., y: 0. })
            .unwrap();
        for _ in 0..hz {
            game.step(&mut world, &[], 1. / hz as f32).unwrap();
            assert!(
                game.motor.grounded(),
                "hz={hz} {:?}",
                world.item(1).unwrap().transform.anchor
            );
            assert!(character_fits(
                &world,
                1,
                world.item(1).unwrap().transform.anchor,
                body
            ));
        }
        let end = world.item(1).unwrap().transform.anchor;
        assert!((end.x - 2.5).abs() < 0.001, "{end:?}");
        assert!((end.z - 0.9).abs() < 0.001, "{end:?}");
    }
}

#[derive(Clone, Debug)]
struct NavigatorDemo {
    movement: Movement,
    status: NavigationStatus,
    grounded: bool,
}
impl GamePlugin for NavigatorDemo {
    type Command = ();
    type Event = ();
    type Error = Error;
    fn info(&self) -> PluginInfo {
        PluginInfo {
            id: "surface-navigation-test",
            version: 1,
        }
    }
    fn validate(&self, world: &dyn WorldView) -> Result<(), Error> {
        self.movement.validate(world)
    }
    fn active_items(&self) -> Vec<u64> {
        self.movement.active_items()
    }
    fn command(&mut self, _: &mut PluginWorld<()>, _: ()) -> Result<(), Error> {
        Ok(())
    }
    fn update(&mut self, world: &mut PluginWorld<()>, tick: Tick) -> Result<(), Error> {
        let frame = self.movement.update(world, tick.seconds())?;
        self.status = frame.actors[0].status;
        self.grounded = frame.actors[0].execution.grounded;
        Ok(())
    }
}

fn navigate(
    world: &mut World,
    motor: Motor,
    start: Vec3,
    goal: Vec3,
    domain: io_traversal::navigation::Domain,
    hz: usize,
    seconds: usize,
) {
    assert!(world.set_pose(1, start, 0.));
    let mut movement = Movement::new(
        domain,
        32,
        vec![ActorBinding {
            motor,
            can_crouch: true,
            familiar_points: vec![],
            steering: SteeringSettings::default(),
        }],
        world,
    )
    .unwrap();
    let reply = movement.request(io_types::Envelope::new(
        io_types::MessageId(1),
        NavigationRequest::Start {
            actor: 1,
            goal: NavigationGoal::Position(goal),
        },
    ));
    assert!(reply.payload.is_ok());
    let mut game = Session::register(
        NavigatorDemo {
            movement,
            status: NavigationStatus::Planning,
            grounded: false,
        },
        world,
    )
    .unwrap();
    for _ in 0..hz * seconds {
        game.step(world, &[], 1. / hz as f32).unwrap();
        let actor = world.item(1).unwrap();
        assert!(character_fits(
            world,
            1,
            actor.transform.anchor,
            actor.character_body.unwrap()
        ));
        assert!(
            game.grounded,
            "lost support at {:?}",
            actor.transform.anchor
        );
        if game.status == NavigationStatus::Arrived {
            break;
        }
    }
    let end = world.item(1).unwrap().transform.anchor;
    assert_eq!(
        game.status,
        NavigationStatus::Arrived,
        "hz={hz} at {end:?}, goal {goal:?}"
    );
    let delta = end - goal;
    assert!(delta.dot(delta).sqrt() < 0.02, "{end:?} != {goal:?}");
}

#[test]
fn planner_and_real_motor_traverse_tilted_box_and_terrain_crest_at_multiple_rates() {
    for hz in [30, 60, 144] {
        let (mut world, game) = fixture();
        let body = world.item(1).unwrap().character_body.unwrap();
        let start = world.item(1).unwrap().transform.anchor;
        let goal = character_support(
            &world,
            Some(1),
            Vec3::new(2., 0., 2.),
            body,
            SupportProbe::new(4., 4.).unwrap(),
        )
        .unwrap()
        .unwrap()
        .anchor;
        let domain = io_traversal::navigation::Domain {
            origin: [-3., -2., 0.],
            size: [6., 4.],
            cell_size: 0.5,
        };
        navigate(&mut world, game.motor.clone(), start, goal, domain, hz, 10);
        navigate(&mut world, game.motor.clone(), goal, start, domain, hz, 10);

        let mut world = World::new(world.space().clone(), vec![world.item(1).unwrap().clone()]);
        world.set_terrain(
            io_world::HeightField::new([0., -2.], 2., 5, 3, [0., 1., 2., 1., 0.].repeat(3))
                .unwrap(),
        );
        let support = |p| {
            character_support(&world, Some(1), p, body, SupportProbe::new(4., 4.).unwrap())
                .unwrap()
                .unwrap()
                .anchor
        };
        let start = support(Vec3::new(0.5, 0., 0.));
        let goal = support(Vec3::new(7.5, 0., 0.));
        navigate(
            &mut world,
            game.motor.clone(),
            start,
            goal,
            io_traversal::navigation::Domain {
                origin: [0., -1., 99.],
                size: [8., 2.],
                cell_size: 0.5,
            },
            hz,
            12,
        );
    }
}

#[test]
fn planner_and_motor_reach_upper_layer_via_an_actual_ramp_not_vertical_snapping() {
    for hz in [30, 60, 144] {
        let (old, game) = fixture();
        let deck = Item {
            id: 3,
            transform: Transform::new(Vec3::new(6., 0., 3.75), Vec3::new(1., 1., 1.), 0.).unwrap(),
            collider: Some(Collider::new(ColliderShape::Box {
                half_extents: Vec3::new(6., 2., 0.25),
            })),
            physics_body: Some(PhysicsBody::new(BodyKind::Static)),
            ..Item::default()
        };
        let mut world = World::new(
            old.space().clone(),
            vec![old.item(1).unwrap().clone(), deck],
        );
        world.set_terrain(
            io_world::HeightField::new([0., -6.], 6., 5, 3, [0., 0., 4., 0., 0.].repeat(3))
                .unwrap(),
        );
        navigate(
            &mut world,
            game.motor.clone(),
            Vec3::new(2., 0., 0.),
            Vec3::new(2., 0., 4.),
            io_traversal::navigation::Domain {
                origin: [1., -4., 0.],
                size: [22., 8.],
                cell_size: 0.5,
            },
            hz,
            40,
        );
    }
}
