use super::*;
use io_locomotion::ClipDefinition;
use io_types::Vec3;
use io_world::{
    AnimationState, BodyKind, CharacterBody, Collider, ColliderShape, Item, PhysicsBody, Space,
    Transform, World,
};

fn fixture() -> (World, Game) {
    let mut hero = Item {
        id: 1,
        renderable: Some(Default::default()),
        animation: Some(AnimationState::looping(0)),
        character_body: Some(CharacterBody {
            radius: 0.35,
            height: 1.9,
            max_slope: 0.8,
        }),
        ..Item::default()
    };
    hero.transform = Transform::new(Vec3::default(), Vec3::new(1., 1., 1.), 0.).unwrap();
    let block = |id, p, size: Vec3| {
        let mut collider = Collider::new(ColliderShape::Box {
            half_extents: size.scaled(0.5),
        });
        collider.offset = size.scaled(0.5);
        Item {
            id,
            transform: Transform::new(p, size, 0.).unwrap(),
            physics_body: Some(PhysicsBody::new(BodyKind::Static)),
            collider: Some(collider),
            ..Item::default()
        }
    };
    let w = World::new(
        Space::new(Vec3::new(100., 100., 30.)),
        vec![
            hero,
            block(2, Vec3::new(-20., -20., -0.5), Vec3::new(40., 40., 0.5)),
            block(3, Vec3::new(4., -2., 1.5), Vec3::new(4., 4., 0.3)),
        ],
    );
    let d = Definition {
        locomotion_profiles: Default::default(),
        player: "hero".into(),
        navigation: None,
        observations: vec![],
        obstacle_observations: None,
        debug_routes: false,
        barrier_cycle: None,
        barrier_cycles: vec![],
        battle: None,
        locomotion: Locomotion {
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
        },
    };
    let clips = Motion::ALL
        .into_iter()
        .enumerate()
        .map(|(i, m)| (m, (i, 1.)))
        .collect();
    let p = Traversal::new(d, 1, clips, &w).unwrap();
    let game = Game::register(p, &w).unwrap();
    (w, game)
}
fn ticks(w: &mut World, g: &mut Game, n: usize) {
    for _ in 0..n {
        g.step(w, &[], 1. / 60.).unwrap();
    }
}

fn attention_route_fixture(threshold: f32) -> (World, Game) {
    let (old, base) = fixture();
    let mut items = old.items()[..2].to_vec();
    items[0].transform.anchor = Vec3::new(-10., 8., 0.);
    let mut npc = items[0].clone();
    npc.id = 4;
    npc.transform = Transform::new(
        Vec3::new(-6., 0., 0.),
        Vec3::new(1., 1., 1.),
        std::f32::consts::FRAC_PI_2,
    )
    .unwrap();
    items.push(npc);
    items.push(Item {
        id: 5,
        collider: Some(Collider::new(ColliderShape::Box {
            half_extents: Vec3::new(0.3, 10., 1.5),
        })),
        physics_body: Some(PhysicsBody::new(BodyKind::Kinematic)),
        transform: Transform::new(Vec3::new(0., 0., 1.5), Vec3::new(0.6, 20., 3.), 0.).unwrap(),
        ..Default::default()
    });
    let world = World::new(old.space().clone(), items);
    let domain = io_traversal::navigation::Domain {
        origin: [-8., -4., 0.],
        size: [16., 8.],
        cell_size: 1.,
    };
    let vision = io_perception::VisionProfile {
        range: 100.,
        fov_degrees: 180.,
        gain_per_second: 8.,
        decay_per_second: 1.,
    };
    let locomotion = base.player.settings().clone();
    let clips: BTreeMap<_, _> = Motion::ALL
        .into_iter()
        .enumerate()
        .map(|(i, m)| (m, (i, 1.)))
        .collect();
    let motor = Motor::new(locomotion.clone(), 4, clips.clone(), &world).unwrap();
    let definition = Definition {
        locomotion_profiles: Default::default(),
        obstacle_observations: None,
        player: "player".into(),
        locomotion,
        navigation: Some(NavigationDefinition {
            domain,
            planning: io_traversal::PlanningMode::Background,
            athletics: None,
            expansions_per_tick: 32,
            agents: vec![NpcDefinition {
                item: "npc".into(),
                goal: [6., 0., 0.],
                locomotion: None,
                locomotion_profile: None,
                athletics: None,
                can_crouch: false,
                familiar_points: vec![],
                on_arrival: OnArrival::Stop,
                pursuit: None,
                steering: Default::default(),
            }],
        }),
        observations: vec![ObservationDefinition {
            observer: "npc".into(),
            target: "gate".into(),
            vision,
            notice_attention: threshold,
        }],
        debug_routes: false,
        barrier_cycle: None,
        barrier_cycles: vec![],
        battle: None,
    };
    let plugin = Traversal::with_npcs(
        definition,
        1,
        clips,
        vec![NpcBinding {
            motor,
            goal: Vec3::new(6., 0., 0.),
            can_crouch: false,
            familiar_points: vec![],
            on_arrival: OnArrival::Stop,
            pursuit: None,
            steering: Default::default(),
        }],
        &world,
    )
    .unwrap()
    .with_observations(
        vec![ObservationBinding {
            observer: 4,
            target: 5,
            label: "gate".into(),
            vision,
            notice_attention: threshold,
        }],
        &world,
    )
    .unwrap();
    let game = Game::register(plugin, &world).unwrap();
    (world, game)
}

fn paced_ticks(w: &mut World, g: &mut Game, count: usize) {
    for _ in 0..count {
        ticks(w, g, 1);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

#[test]
fn discovered_opening_replans_without_explicit_observation_bindings() {
    let (mut world, base) = attention_route_fixture(1.);
    let mut plugin = (*base).clone();
    plugin.observations.clear();
    plugin.expected_observations = 0;
    plugin.obstacle_observations = Some(discovery::ObstacleObservations::new(
        ObstacleObservationSettings {
            vision: io_perception::VisionProfile {
                range: 100.,
                fov_degrees: 200.,
                gain_per_second: 8.,
                decay_per_second: 1.,
            },
            notice_attention: 0.01,
            ..Default::default()
        },
        std::iter::once(4),
    ));
    let mut game = Game::register(plugin, &world).unwrap();
    paced_ticks(&mut world, &mut game, 400);
    assert!(game.observations().is_empty());
    assert_eq!(
        game.movement.as_ref().unwrap().actor(4).unwrap().status,
        Status::Unreachable
    );
    let before = game.discovery_stats().unwrap();
    assert!(before.notices > 0);
    let ticket = game.movement.as_ref().unwrap().actor(4).unwrap().ticket;
    assert!(world.set_kinematic_target(5, Vec3::new(0., 12., 1.5), Default::default()));
    paced_ticks(&mut world, &mut game, 900);
    let actor = game.movement.as_ref().unwrap().actor(4).unwrap();
    assert_eq!(actor.ticket, ticket);
    assert_eq!(actor.status, Status::Arrived);
    assert!(game.discovery_stats().unwrap().reconsiderations > before.reconsiderations);
}

#[test]
fn discovered_obstacle_requests_a_detour_before_the_motor_reaches_it() {
    let (old, base) = attention_route_fixture(1.);
    let mut items = old.items().to_vec();
    let gate = items.iter_mut().find(|i| i.id == 5).unwrap();
    gate.collider = Some(Collider::new(ColliderShape::Box {
        half_extents: Vec3::new(0.3, 1.2, 1.5),
    }));
    gate.transform.anchor.y = 12.;
    gate.transform.size = Vec3::new(0.6, 2.4, 3.);
    let mut world = World::new(old.space().clone(), items);
    let mut plugin = (*base).clone();
    plugin.observations.clear();
    plugin.expected_observations = 0;
    plugin.obstacle_observations = Some(discovery::ObstacleObservations::new(
        ObstacleObservationSettings::default(),
        std::iter::once(4),
    ));
    let mut game = Game::register(plugin, &world).unwrap();
    for _ in 0..400 {
        paced_ticks(&mut world, &mut game, 1);
        if game.movement.as_ref().unwrap().actor(4).unwrap().status == Status::Following {
            break;
        }
    }
    assert_eq!(
        game.movement.as_ref().unwrap().actor(4).unwrap().status,
        Status::Following
    );
    assert!(world.item(4).unwrap().transform.anchor.x < -5.);
    let before = game.discovery_stats().unwrap().reconsiderations;
    assert!(world.set_kinematic_target(5, Vec3::new(0., 0., 1.5), Default::default()));
    let mut proactive = false;
    let mut detoured = false;
    for _ in 0..900 {
        paced_ticks(&mut world, &mut game, 1);
        let p = world.item(4).unwrap().transform.anchor;
        if game.discovery_stats().unwrap().reconsiderations > before && !proactive {
            assert!(p.x < -2., "notice must precede contact: {p:?}");
            proactive = true;
        }
        detoured |= p.x.abs() < 1. && p.y.abs() > 1.5;
        assert!(
            p.x.abs() >= 0.649 || p.y.abs() >= 1.549,
            "no collision bypass: {p:?}"
        );
    }
    assert!(proactive && detoured);
    assert_eq!(
        game.movement.as_ref().unwrap().actor(4).unwrap().status,
        Status::Arrived
    );
}

#[test]
fn observed_opening_replans_only_after_attention_qualifies_and_keeps_the_ticket() {
    let (mut world, mut game) = attention_route_fixture(1.);
    paced_ticks(&mut world, &mut game, 400);
    let before = game.movement.as_ref().unwrap().planning_stats();
    let ticket = game.movement.as_ref().unwrap().actor(4).unwrap().ticket;
    assert_eq!(
        game.movement.as_ref().unwrap().actor(4).unwrap().status,
        Status::Unreachable
    );
    assert!(world.set_kinematic_target(5, Vec3::new(0., 12., 1.5), Default::default()));
    paced_ticks(&mut world, &mut game, 120);
    assert_eq!(
        game.movement.as_ref().unwrap().planning_stats().submitted,
        before.submitted,
        "unqualified geometry changes must not initiate planning"
    );
    assert!(game.observations()[0].notice().is_none());
    game.command(
        &mut world,
        Input::TuneAttention(AttentionSettings {
            observer: 4,
            target: 5,
            fov_degrees: 180.,
            notice_attention: 0.01,
        }),
    )
    .unwrap();
    paced_ticks(&mut world, &mut game, 700);
    let movement = game.movement.as_ref().unwrap();
    assert_eq!(movement.actor(4).unwrap().ticket, ticket);
    assert_eq!(
        movement.actor(4).unwrap().status,
        Status::Arrived,
        "attention={} evidence={} planning={:?} npc={:?} gate={:?} events={:?}",
        game.observations()[0].attention(),
        game.observations()[0].evidence(),
        movement.planning_stats(),
        world.item(4).unwrap().transform,
        world.item(5).unwrap().transform,
        game.events()
    );
    assert!(movement.planning_stats().submitted > before.submitted);
    assert!(world.item(4).unwrap().transform.anchor.x > 5.9);
}

#[test]
fn route_attention_filters_unrelated_corridors_and_other_observers() {
    use io_game::stage::Stage;
    use io_perception::pipeline::{ItemNotice, NoticeKind};
    use io_types::Bounds;
    let bounds = Bounds {
        min: Vec3::new(3., -0.5, 0.),
        max: Vec3::new(4., 0.5, 2.),
    };
    let notice = ItemNotice {
        observer: 4,
        target: 5,
        kind: NoticeKind::Changed,
        position: Vec3::new(3., 8., 0.),
        attention: 0.9,
        previous_bounds: Some(bounds),
        bounds: Bounds {
            min: Vec3::new(3., 8., 0.),
            max: Vec3::new(4., 9., 2.),
        },
    };
    let route = [Vec3::new(6., 0., 0.)];
    let input = |notice, actor| RouteAttentionRequest {
        notice,
        ticket: NavigationTicket { actor, revision: 3 },
        status: Status::Following,
        position: Vec3::default(),
        route: &route,
        body: CharacterBody {
            radius: 0.35,
            height: 1.9,
            max_slope: 0.8,
        },
    };
    assert!(
        RouteAttentionStage.run(input(notice, 4)).unwrap().is_some(),
        "old noticed bounds matter when an obstacle moves away"
    );
    assert!(RouteAttentionStage
        .run(input(
            ItemNotice {
                previous_bounds: None,
                ..notice
            },
            4
        ))
        .unwrap()
        .is_none());
    assert!(RouteAttentionStage.run(input(notice, 99)).is_err());
}

#[test]
fn four_occupied_patrol_corners_do_not_form_a_circular_planning_wait() {
    let (old, base) = attention_route_fixture(1.);
    let corners = [
        Vec3::new(-4., -4., 0.),
        Vec3::new(4., -4., 0.),
        Vec3::new(4., 4., 0.),
        Vec3::new(-4., 4., 0.),
    ];
    let mut items = old.items()[..2].to_vec();
    for (i, position) in corners.iter().enumerate() {
        let mut item = old.item(4).unwrap().clone();
        item.id = 4 + i as u64;
        item.transform = Transform::new(*position, item.transform.size, 0.).unwrap();
        items.push(item);
    }
    let mut world = World::new(old.space().clone(), items);
    let settings = base.player.settings().clone();
    let clips: BTreeMap<_, _> = Motion::ALL
        .into_iter()
        .enumerate()
        .map(|(i, m)| (m, (i, 1.)))
        .collect();
    let bindings = (4..8)
        .map(|id| ActorBinding {
            motor: Motor::new(settings.clone(), id, clips.clone(), &world).unwrap(),
            can_crouch: false,
            familiar_points: vec![],
            steering: Default::default(),
        })
        .collect::<Vec<_>>();
    let domain = io_traversal::navigation::Domain {
        origin: [-8., -8., 0.],
        size: [16., 16.],
        cell_size: 1.,
    };
    let mut movement = TraversalService::Surface(
        io_locomotion::Movement::new(domain, 32, bindings, &world).unwrap(),
    );
    let mut plugin = (*base).clone();
    plugin.domain = Some(domain);
    plugin.observations.clear();
    plugin.expected_observations = 0;
    let template = plugin.behaviors[0].clone();
    plugin.behaviors.clear();
    for i in 0..4 {
        let actor = 4 + i as u64;
        let goal = corners[(i + 1) % 4];
        let ticket = submit(
            &mut movement,
            &mut plugin.next_message,
            NavigationRequest::Start {
                actor,
                goal: NavigationGoal::Position(goal),
            },
        )
        .unwrap();
        plugin.behaviors.push(Behavior {
            actor,
            ticket,
            patrol_goal: goal,
            requested: Some(NavigationGoal::Position(goal)),
            ..template.clone()
        });
    }
    plugin.movement = Some(movement);
    let mut game = Game::register(plugin, &world).unwrap();
    ticks(&mut world, &mut game, 900);
    for i in 0..4 {
        let actor = 4 + i as u64;
        assert_eq!(
            game.movement.as_ref().unwrap().actor(actor).unwrap().status,
            Status::Arrived
        );
        let delta = world.item(actor).unwrap().transform.anchor - corners[(i + 1) % 4];
        assert!(delta.dot(delta) < 0.001);
    }
}

#[test]
fn standing_is_blocked_crouching_enters_and_release_waits_for_clearance() {
    let (mut w, mut g) = fixture();
    g.command(&mut w, Command::Move { x: 1., y: 0. }.into())
        .unwrap();
    ticks(&mut w, &mut g, 180);
    assert!(w.item(1).unwrap().transform.anchor.x < 3.651);
    g.command(&mut w, Command::Crouch(true).into()).unwrap();
    ticks(&mut w, &mut g, 90);
    assert!(w.item(1).unwrap().transform.anchor.x > 4.5);
    g.command(&mut w, Command::Crouch(false).into()).unwrap();
    ticks(&mut w, &mut g, 60);
    assert!(g.crouched());
    assert_eq!(w.item(1).unwrap().character_body.unwrap().height, 1.15);
    ticks(&mut w, &mut g, 300);
    assert!(!g.crouched());
}
#[test]
fn jumping_moves_the_body_lands_and_cannot_repeat_in_midair() {
    let (mut w, mut g) = fixture();
    ticks(&mut w, &mut g, 1);
    g.command(&mut w, Command::Jump.into()).unwrap();
    ticks(&mut w, &mut g, 8);
    let speed = g.vertical_speed();
    assert!(speed > 0.);
    assert!(w.item(1).unwrap().transform.anchor.z > 0.4);
    g.command(&mut w, Command::Jump.into()).unwrap();
    ticks(&mut w, &mut g, 1);
    assert!(g.vertical_speed() < speed);
    ticks(&mut w, &mut g, 100);
    assert!(g.grounded());
    assert!(w.item(1).unwrap().transform.anchor.z.abs() < 0.001);
    assert!(g.events().iter().any(|e| matches!(
        e.payload,
        Event::Motion {
            actor: 1,
            event: MotionEvent::Landed
        }
    )));
}
#[test]
fn retained_offscreen_actor_advances_once_and_invalid_input_preserves_intent() {
    let (mut w, mut g) = fixture();
    g.command(&mut w, Command::Move { x: 0., y: 1. }.into())
        .unwrap();
    assert_eq!(
        g.command(&mut w, Command::Move { x: f32::NAN, y: 0. }.into()),
        Err(Error::InvalidInput)
    );
    ticks(&mut w, &mut g, 60);
    assert!((w.item(1).unwrap().transform.anchor.y - 2.1).abs() < 0.001);
    assert_eq!(w.item(1).unwrap().simulated_ticks, 60);
    let snapshot = w.snapshot();
    ticks(&mut w, &mut g, 10);
    assert!(w.item(1).unwrap().transform.anchor.y > snapshot.item(1).unwrap().transform.anchor.y);
}

fn moving_fixture() -> (World, Game) {
    let (world, game) = fixture();
    let mut items = world.items().to_vec();
    for (id, x) in [(4, -10.), (5, 10.)] {
        let mut collider = Collider::new(ColliderShape::Box {
            half_extents: Vec3::new(0.5, 0.5, 0.5),
        });
        collider.offset = Vec3::new(0.5, 0.5, 0.5);
        items.push(Item {
            id,
            transform: Transform::new(Vec3::new(x, -10., 0.), Vec3::new(1., 1., 1.), 0.).unwrap(),
            collider: Some(collider),
            physics_body: Some(PhysicsBody::new(BodyKind::Kinematic)),
            ..Item::default()
        });
    }
    let world = World::new(world.space().clone(), items);
    let mut plugin: Traversal = (*game).clone();
    plugin.expected_barriers = vec![
        BarrierDefinition {
            item: "a".into(),
            alternate: [-6., -10., 0.],
            interval_seconds: 0.,
            travel_seconds: 1.,
        },
        BarrierDefinition {
            item: "b".into(),
            alternate: [14., -10., 0.],
            interval_seconds: 0.,
            travel_seconds: 1.,
        },
    ];
    assert!(plugin.clone().with_barriers(&[4, 4], &world).is_err());
    let plugin = plugin.with_barriers(&[4, 5], &world).unwrap();
    let game = Game::register(plugin, &world).unwrap();
    (world, game)
}

#[test]
fn multiple_movers_are_active_offscreen_and_advance_smoothly() {
    let (mut w, mut g) = moving_fixture();
    assert!(g.active_items().contains(&4) && g.active_items().contains(&5));
    ticks(&mut w, &mut g, 30);
    assert!((w.item(4).unwrap().transform.anchor.x + 8.).abs() < 0.1);
    assert!((w.item(5).unwrap().transform.anchor.x - 12.).abs() < 0.1);
    assert!(w.navigation_revision() > 0);
}

#[test]
fn moving_fixture_stops_before_crossing_a_character() {
    let (mut w, mut g) = moving_fixture();
    assert!(w.set_pose(1, Vec3::new(-7.5, -9.5, 0.), 0.));
    ticks(&mut w, &mut g, 120);
    assert!(w.item(4).unwrap().transform.anchor.x < -8.8);
    assert!(io_world::character_segment_clear(
        &w,
        Some(1),
        w.item(1).unwrap().transform.anchor,
        w.item(1).unwrap().transform.anchor,
        w.item(1).unwrap().character_body.unwrap()
    ));
}
