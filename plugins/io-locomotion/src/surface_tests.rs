use super::surface::{
    connection, live_supported_segment, supported_segment, Action, Agent, MovementProfile,
    Navigation,
};
use io_traversal::navigation::{Cell, Domain, Status};
use io_types::Vec3;
use io_world::{
    character_space_fits, BodyKind, Collider, ColliderShape, PhysicsBody, Transform, World,
};
use io_world::{Item, Space, WorldView};

fn block(id: u64, position: Vec3, size: Vec3) -> Item {
    let mut collider = Collider::new(ColliderShape::Box {
        half_extents: size.scaled(0.5),
    });
    collider.offset = size.scaled(0.5);
    Item {
        id,
        transform: Transform::new(position, size, 0.).unwrap(),
        collider: Some(collider),
        physics_body: Some(PhysicsBody::new(if id == 2 {
            BodyKind::Kinematic
        } else {
            BodyKind::Static
        })),
        ..Item::default()
    }
}
fn fixture() -> (World, Navigation, MovementProfile) {
    let world = World::new(
        Space::new(Vec3::new(20., 20., 10.)),
        vec![
            block(1, Vec3::new(-1., -1., -0.5), Vec3::new(12., 4., 0.5)),
            block(2, Vec3::new(3., -1., 1.5), Vec3::new(2., 4., 0.5)),
        ],
    );
    let nav = Navigation::new(Domain {
        origin: [0., 0., 0.],
        size: [8., 2.],
        cell_size: 0.5,
    })
    .unwrap();
    let profile = MovementProfile {
        radius: 0.35,
        max_slope: 0.8,
        standing_height: 1.9,
        crouch_height: Some(1.1),
        walk_speed: 3.,
        crouch_speed: 1.,
    };
    (world, nav, profile)
}

#[test]
fn destination_checks_use_body_clearance_support_domain_and_live_actors() {
    let (world, nav, profile) = fixture();
    let goal = Vec3::new(4., 1., 0.);
    let crouching = Agent::new(profile, goal).unwrap();
    let standing = Agent::for_actor(
        99,
        MovementProfile {
            crouch_height: None,
            ..profile
        },
        goal,
    )
    .unwrap();
    assert!(nav.destination_available(&world, &crouching, goal));
    assert!(!nav.destination_available(&world, &standing, goal));
    assert!(!nav.destination_available(&world, &standing, Vec3::new(-2., 1., 0.)));
    assert!(!nav.destination_available(&world, &standing, Vec3::new(1., 1., 1.)));
    let mut items = world.items().to_vec();
    items.push(Item {
        id: 3,
        character_body: Some(profile.shape(Action::Walk)),
        transform: Transform::new(Vec3::new(1., 1., 0.), Vec3::new(1., 1., 1.), 0.).unwrap(),
        ..Default::default()
    });
    let world = World::new(world.space().clone(), items);
    assert!(!nav.destination_available(&world, &standing, Vec3::new(1., 1., 0.)));
    let empty = World::new(world.space().clone(), vec![]);
    assert!(!nav.destination_available(&empty, &standing, Vec3::new(1., 1., 0.)));
}
fn solve(nav: &mut Navigation, world: &World, profile: MovementProfile) -> Agent {
    let mut agent = Agent::new(profile, Vec3::new(7., 1., 0.)).unwrap();
    for _ in 0..100 {
        nav.advance(world, &mut agent, Vec3::new(1., 1., 0.), 8);
        if agent.status() != Status::Planning {
            return agent;
        }
    }
    panic!("bounded domain search did not finish");
}
#[test]
fn short_and_tall_agents_share_geometry_not_unsafe_clearance_answers() {
    let (world, mut nav, profile) = fixture();
    let tall = solve(&mut nav, &world, profile);
    let route = tall.route().expect("missing route");
    assert!(route.iter().any(|p| p.action == Action::Crouch));
    let short = MovementProfile {
        standing_height: 1.2,
        crouch_height: None,
        ..profile
    };
    let short = solve(&mut nav, &world, short);
    let route = short.route().expect("missing route");
    assert!(route.iter().all(|p| p.action == Action::Walk));
    let unable = solve(
        &mut nav,
        &world,
        MovementProfile {
            crouch_height: None,
            ..profile
        },
    );
    assert_eq!(unable.status(), Status::Unreachable);
}
#[test]
fn shared_cache_budget_and_individual_knowledge() {
    let (world, mut nav, profile) = fixture();
    let first = solve(&mut nav, &world, profile);
    let queries = nav.stats().clearance_queries;
    let mut second = Agent::new(profile, Vec3::new(7., 1., 0.)).unwrap();
    assert_eq!(second.knowledge().known_cells(), 0);
    let before = nav.stats().expanded;
    nav.advance(&world, &mut second, Vec3::new(1., 1., 0.), 1);
    assert!(nav.stats().expanded - before <= 1);
    assert!(second.knowledge().known_cells() < first.knowledge().known_cells());
    let second = solve(&mut nav, &world, profile);
    assert_eq!(second.status(), Status::Following);
    assert_eq!(queries, nav.stats().clearance_queries);
    assert!(nav.stats().cache_hits > 0);
}
#[test]
fn geometry_changes_invalidate_but_character_movement_does_not() {
    let (mut world, mut nav, profile) = fixture();
    let mut agent = solve(&mut nav, &world, profile);
    let before = world.navigation_revision();
    assert!(world.set_kinematic_target(2, Vec3::new(3., -1., 0.), io_types::Rotation::default()));
    world.simulate(&[], 1. / 60.);
    assert_ne!(before, world.navigation_revision());
    nav.advance(&world, &mut agent, Vec3::new(1., 1., 0.), 0);
    assert_eq!(
        agent.status(),
        Status::Following,
        "cache invalidation is not a route cancellation"
    );
    agent.retry();
    for _ in 0..100 {
        nav.advance(&world, &mut agent, Vec3::new(1., 1., 0.), 8);
    }
    assert_eq!(agent.status(), Status::Unreachable);
    assert_eq!(nav.stats().invalidations, 1);
    let mut actor = Item {
        id: 3,
        character_body: Some(profile.shape(Action::Walk)),
        ..Item::default()
    };
    actor.transform.anchor = Vec3::new(1., 1., 0.);
    let mut world = World::new(world.space().clone(), vec![actor]);
    let revision = world.navigation_revision();
    world.set_pose(3, Vec3::new(2., 1., 0.), 0.);
    assert_eq!(revision, world.navigation_revision());
    assert_eq!(revision, world.snapshot().navigation_revision());
}
#[test]
fn no_floor_no_path_and_wrong_level_is_not_a_teleport() {
    let (world, mut nav, profile) = fixture();
    let mut agent = Agent::new(profile, Vec3::new(7., 1., 3.)).unwrap();
    assert!(nav
        .advance(&world, &mut agent, Vec3::new(1., 1., 0.), 8)
        .is_none());
    assert_eq!(agent.status(), Status::Unreachable);
    let empty = World::new(world.space().clone(), vec![]);
    // A cache belongs to one world lifetime. Construct a new service for a new world.
    let mut nav = Navigation::new(nav.configuration()).unwrap();
    assert_eq!(
        solve(&mut nav, &empty, profile).status(),
        Status::Unreachable
    );
}
#[test]
fn invalid_profiles_and_domains_fail_before_searching() {
    let (_, _, profile) = fixture();
    for profile in [
        MovementProfile {
            radius: f32::NAN,
            ..profile
        },
        MovementProfile {
            max_slope: f32::NAN,
            ..profile
        },
        MovementProfile {
            crouch_height: Some(3.),
            ..profile
        },
    ] {
        assert!(Agent::new(profile, Vec3::default()).is_err());
    }
    assert!(Navigation::new(Domain {
        origin: [0.; 3],
        size: [1000., 1000.],
        cell_size: 0.2
    })
    .is_err());
}

#[test]
fn slope_limits_are_part_of_shared_clearance_identity() {
    let (_, _, profile) = fixture();
    let limited = MovementProfile {
        max_slope: 0.1,
        ..profile
    };
    assert_ne!(profile.key(), limited.key());
    assert_eq!(limited.shape(Action::Walk).max_slope, 0.1);
    assert_eq!(limited.shape(Action::Crouch).max_slope, 0.1);
}

#[test]
fn body_width_is_numeric_and_support_is_required_between_samples() {
    let (world, nav, profile) = fixture();
    let mut items = world.items().to_vec();
    items.extend([
        block(4, Vec3::new(3., -1., 0.), Vec3::new(2., 1.75, 3.)),
        block(5, Vec3::new(3., 1.25, 0.), Vec3::new(2., 1.75, 3.)),
    ]);
    let world = World::new(world.space().clone(), items);
    let mut nav = Navigation::new(nav.configuration()).unwrap();
    assert_eq!(
        solve(
            &mut nav,
            &world,
            MovementProfile {
                radius: 0.2,
                ..profile
            }
        )
        .status(),
        Status::Following
    );
    assert_eq!(
        solve(&mut nav, &world, profile).status(),
        Status::Unreachable
    );
    let world = World::new(
        world.space().clone(),
        vec![
            block(1, Vec3::new(-1., -1., -0.5), Vec3::new(3., 4., 0.5)),
            block(3, Vec3::new(3., -1., -0.5), Vec3::new(9., 4., 0.5)),
        ],
    );
    let mut nav = Navigation::new(nav.configuration()).unwrap();
    assert_eq!(
        solve(&mut nav, &world, profile).status(),
        Status::Unreachable
    );
}

#[test]
fn familiar_points_are_not_a_boundary_and_retarget_keeps_knowledge() {
    let (world, mut nav, profile) = fixture();
    let start = Vec3::new(1., 1., 0.);
    let mut agent = Agent::new(profile, Vec3::new(7., 1., 0.)).unwrap();
    nav.familiarize(&world, &mut agent, &[start]);
    assert_eq!(agent.knowledge().known_cells(), 1);
    nav.advance(&world, &mut agent, start, 0);
    assert_eq!(agent.knowledge().known_cells(), 1);
    for _ in 0..100 {
        nav.advance(&world, &mut agent, start, 8);
    }
    assert_eq!(agent.status(), Status::Following);
    let learned = agent.knowledge().known_cells();
    assert!(learned > 1);
    assert!(agent.set_goal(Vec3::new(f32::NAN, 0., 0.)).is_err());
    assert_eq!(agent.status(), Status::Following);
    agent.set_goal(start).unwrap();
    assert_eq!(agent.knowledge().known_cells(), learned);
    nav.advance(&world, &mut agent, start, 8);
    assert_eq!(agent.status(), Status::Arrived);
}

#[test]
fn terrain_replacement_invalidates_cached_support_and_old_snapshot_stays_stable() {
    let (mut world, mut nav, profile) = fixture();
    let mut agent = solve(&mut nav, &world, profile);
    let snapshot = world.snapshot();
    world.set_terrain(io_world::HeightField::new([-1., -1.], 10., 2, 2, vec![2.; 4]).unwrap());
    assert_ne!(snapshot.navigation_revision(), world.navigation_revision());
    nav.advance(&world, &mut agent, Vec3::new(1., 1., 0.), 8);
    assert!(
        !nav.steering_segment_clear(
            &world,
            &agent,
            Vec3::new(1., 1., 0.),
            Vec3::new(1.05, 1., 0.),
            Action::Walk
        ),
        "live support still rejects the old route"
    );
    agent.retry();
    nav.advance(&world, &mut agent, Vec3::new(1., 1., 0.), 8);
    assert_eq!(agent.status(), Status::Unreachable);
}

#[test]
fn diagonal_edges_check_the_whole_body_not_just_the_endpoints() {
    let (world, nav, profile) = fixture();
    let mut items = world.items().to_vec();
    items.push(block(3, Vec3::new(1.45, 1.45, 0.), Vec3::new(0.1, 0.1, 3.)));
    let world = World::new(world.space().clone(), items);
    let a = Vec3::new(1., 1., 0.);
    let b = Vec3::new(2., 2., 0.);
    assert!(character_space_fits(&world, a, profile.shape(Action::Walk)));
    assert!(character_space_fits(&world, b, profile.shape(Action::Walk)));
    assert!(connection(&world, profile, a, b).is_none());
    let nav = Navigation::new(nav.configuration()).unwrap();
    let mut provider = crate::surface::SurfaceRoutes::new(nav.configuration()).unwrap();
    let sample = provider.connectors(&world, profile, a, Cell(2, 2))[0].0;
    assert!(provider
        .edge(&world, profile, sample, Cell(3, 3))
        .is_empty());

    let w = World::new(
        world.space().clone(),
        vec![
            block(1, Vec3::new(-5., -5., -0.5), Vec3::new(15., 15., 0.5)),
            block(3, Vec3::new(2., -1.5, 0.), Vec3::new(1., 3., 3.)),
        ],
    );
    // Both endpoints fit; a very short corner intersection lies between old r/4 samples.
    let a = Vec3::new(1.6476444, -1.8465308, 0.);
    let b = Vec3::new(3.5, -2., 0.);
    assert!(character_space_fits(&w, a, profile.shape(Action::Walk)));
    assert!(character_space_fits(&w, b, profile.shape(Action::Walk)));
    assert!(connection(&w, profile, a, b).is_none());
}

#[test]
fn refinement_preserves_stance_and_defers_actor_occupancy_to_immediate_execution() {
    let (world, mut nav, profile) = fixture();
    let mut agent = Agent::for_actor(99, profile, Vec3::new(7., 1., 0.)).unwrap();
    let mut position = Vec3::new(1., 1., 0.);
    for _ in 0..100 {
        nav.advance(&world, &mut agent, position, 16);
        if agent.status() != Status::Planning {
            break;
        }
    }
    nav.refine_route(&world, &mut agent, position, 6.).unwrap();
    let first = nav.prepared_target(&mut agent, position, 6.).unwrap();
    assert_eq!(first.action, Action::Walk);
    let mut entered_crouch = false;
    for _ in 0..32 {
        let p = nav.prepared_target(&mut agent, position, 6.).unwrap();
        if p.action == Action::Crouch {
            entered_crouch = true;
            break;
        }
        position = p.position;
        nav.advance(&world, &mut agent, position, 0);
    }
    assert!(entered_crouch);
    let mut items = world.items().to_vec();
    items.push(Item {
        id: 88,
        character_body: Some(profile.shape(Action::Crouch)),
        transform: Transform::new(Vec3::new(4., 1., 0.), Vec3::new(1., 1., 1.), 0.).unwrap(),
        ..Default::default()
    });
    let blocked = World::new(world.space().clone(), items);
    assert!(nav.prepared_target(&mut agent, position, 6.).is_some());
    assert_eq!(agent.status(), Status::Following);
    assert!(!nav.steering_segment_clear(
        &blocked,
        &agent,
        Vec3::new(3.2, 1., 0.),
        Vec3::new(3.4, 1., 0.),
        Action::Crouch
    ));
}

fn plan_between(
    nav: &mut Navigation,
    world: &World,
    profile: MovementProfile,
    start: Vec3,
    goal: Vec3,
) -> Agent {
    let mut agent = Agent::new(profile, goal).unwrap();
    for _ in 0..2048 {
        let before = nav.stats().expanded;
        nav.advance(world, &mut agent, start, 3);
        assert!(nav.stats().expanded - before <= 3);
        if agent.status() != Status::Planning {
            return agent;
        }
    }
    panic!("search did not finish");
}

fn supported(world: &World, profile: MovementProfile, p: Vec3) -> Vec3 {
    io_world::character_support(
        world,
        None,
        p,
        profile.shape(Action::Walk),
        io_world::SupportProbe::new(4., 4.).unwrap(),
    )
    .unwrap()
    .unwrap()
    .anchor
}

#[test]
fn terrain_layers_follow_slopes_and_crests_instead_of_domain_origin_height() {
    let (_, _, profile) = fixture();
    let mut world = World::new(Space::new(Vec3::new(20., 20., 20.)), vec![]);
    world.set_terrain(
        io_world::HeightField::new([0., -2.], 2., 5, 3, [0., 1., 2., 1., 0.].repeat(3)).unwrap(),
    );
    let mut nav = Navigation::new(Domain {
        origin: [0., -1., 99.],
        size: [8., 2.],
        cell_size: 0.5,
    })
    .unwrap();
    let start = supported(&world, profile, Vec3::new(0.5, 0., 0.));
    let goal = supported(&world, profile, Vec3::new(7.5, 0., 0.));
    let agent = plan_between(&mut nav, &world, profile, start, goal);
    let steps = agent.route().expect("missing route");
    assert!(steps.iter().any(|p| p.position.z > 1.9));
    let mut previous = start;
    for step in steps.iter() {
        assert!(supported_segment(
            &world,
            profile,
            previous,
            step.position,
            step.action
        ));
        previous = step.position;
    }
    let queries = nav.stats().clearance_queries;
    assert_eq!(
        plan_between(&mut nav, &world, profile, start, goal).status(),
        Status::Following
    );
    assert_eq!(queries, nav.stats().clearance_queries);
    let limited = MovementProfile {
        max_slope: 0.1,
        ..profile
    };
    assert_eq!(
        plan_between(&mut nav, &world, limited, start, goal).status(),
        Status::Unreachable
    );
}

#[test]
fn stacked_floors_share_xy_but_never_nodes_edges_or_shortcuts() {
    let (_, mut nav, profile) = fixture();
    let world = World::new(
        Space::new(Vec3::new(20., 20., 20.)),
        vec![
            block(1, Vec3::new(-1., -1., -0.5), Vec3::new(12., 4., 0.5)),
            block(3, Vec3::new(-1., -1., 3.5), Vec3::new(12., 4., 0.5)),
        ],
    );
    for z in [0., 4.] {
        let agent = plan_between(
            &mut nav,
            &world,
            profile,
            Vec3::new(1., 1., z),
            Vec3::new(7., 1., z),
        );
        let steps = agent.route().expect("missing route");
        assert!(steps.iter().all(|p| (p.position.z - z).abs() < 0.003));
    }
    let mut familiar = Agent::new(profile, Vec3::new(1., 1., 0.)).unwrap();
    nav.familiarize(
        &world,
        &mut familiar,
        &[Vec3::new(1., 1., 0.), Vec3::new(1., 1., 4.)],
    );
    assert_eq!(familiar.knowledge().known_cells(), 2);
    assert_eq!(
        plan_between(
            &mut nav,
            &world,
            profile,
            Vec3::new(1., 1., 0.),
            Vec3::new(1., 1., 4.)
        )
        .status(),
        Status::Unreachable
    );
    assert!(!supported_segment(
        &world,
        profile,
        Vec3::new(1., 1., 0.),
        Vec3::new(1., 1., 4.),
        Action::Walk
    ));
}

#[test]
fn actor_on_terrain_crest_blocks_the_curved_path_not_just_its_chord() {
    let (_, mut nav, profile) = fixture();
    let mut world = World::new(Space::new(Vec3::new(20., 20., 20.)), vec![]);
    world.set_terrain(
        io_world::HeightField::new([0., -2.], 2., 5, 3, [0., 1., 2., 1., 0.].repeat(3)).unwrap(),
    );
    let start = supported(&world, profile, Vec3::new(1., 0., 0.));
    let goal = supported(&world, profile, Vec3::new(7., 0., 0.));
    let blocker = Item {
        id: 8,
        character_body: Some(profile.shape(Action::Walk)),
        transform: Transform::new(Vec3::new(4., 0., 2.), Vec3::new(1., 1., 1.), 0.).unwrap(),
        ..Default::default()
    };
    let terrain = world.terrain().unwrap().clone();
    let mut world = World::new(world.space().clone(), vec![blocker]);
    world.set_terrain(terrain);
    assert!(supported_segment(
        &world,
        profile,
        start,
        goal,
        Action::Walk
    ));
    assert!(!live_supported_segment(
        &world,
        99,
        profile.shape(Action::Walk),
        start,
        goal
    ));
    let agent = Agent::for_actor(99, profile, goal).unwrap();
    assert!(nav.steering_segment_clear(
        &world,
        &agent,
        start,
        start + Vec3::new(0.1, 0., 0.),
        Action::Walk
    ));
    assert!(!nav.steering_segment_clear(
        &world,
        &agent,
        start,
        Vec3::new(5., 0., start.z),
        Action::Walk
    ));
}

#[test]
fn approach_offsets_follow_the_references_surface_and_preserve_its_level() {
    let (_, mut nav, profile) = fixture();
    let mut world = World::new(Space::new(Vec3::new(20., 20., 20.)), vec![]);
    world.set_terrain(
        io_world::HeightField::new([0., -2.], 2., 5, 3, [0., 1., 2., 1., 0.].repeat(3)).unwrap(),
    );
    let reference = supported(&world, profile, Vec3::new(1., 1., 0.));
    let agent = Agent::new(profile, reference).unwrap();
    let offset = nav
        .offset_destination(&world, &agent, reference, [1., 0.])
        .unwrap();
    assert!(offset.z > reference.z + 0.4);
    assert!(supported_segment(
        &world,
        profile,
        reference,
        offset,
        Action::Walk
    ));
    assert!(nav
        .offset_destination(&world, &agent, reference, [f32::NAN, 0.])
        .is_none());
    assert!(nav
        .offset_destination(&world, &agent, reference, [21., 0.])
        .is_none());
    let world = World::new(
        world.space().clone(),
        vec![
            block(1, Vec3::new(-1., -1., -0.5), Vec3::new(12., 4., 0.5)),
            block(3, Vec3::new(-1., -1., 3.5), Vec3::new(12., 4., 0.5)),
        ],
    );
    nav = Navigation::new(nav.configuration()).unwrap();
    for z in [0., 4.] {
        let reference = Vec3::new(1., 1., z);
        let offset = nav
            .offset_destination(&world, &agent, reference, [1., 0.])
            .unwrap();
        assert!((offset.z - z).abs() < 0.003);
    }
}
