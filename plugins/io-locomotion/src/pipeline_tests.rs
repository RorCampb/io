use crate::{pipeline::*, surface::Scenery};
use io_game::stage::{Stage, Then};
use io_types::Vec3;
use io_world::*;
use std::{cell::Cell, time::Instant};

fn shape() -> CharacterBody {
    CharacterBody {
        radius: 0.35,
        height: 1.9,
        max_slope: 0.8,
    }
}
fn fixture() -> World {
    let mut items = vec![Item {
        id: 1,
        physics_body: Some(PhysicsBody::new(BodyKind::Static)),
        collider: Some(Collider::new(ColliderShape::Box {
            half_extents: Vec3::new(20., 20., 0.5),
        })),
        transform: Transform::new(Vec3::new(0., 0., -0.5), Vec3::new(1., 1., 1.), 0.).unwrap(),
        ..Default::default()
    }];
    // Nearby scenery exercises the same broad-phase candidates on repeated checks.
    for i in 0..24 {
        items.push(Item {
            id: i + 2,
            physics_body: Some(PhysicsBody::new(BodyKind::Static)),
            collider: Some(Collider::new(ColliderShape::Box {
                half_extents: Vec3::new(0.2, 0.2, 1.),
            })),
            transform: Transform::new(Vec3::new(i as f32 * 0.5, 4., 1.), Vec3::new(1., 1., 1.), 0.)
                .unwrap(),
            ..Default::default()
        });
    }
    items.push(Item {
        id: 99,
        character_body: Some(shape()),
        ..Default::default()
    });
    items.push(Item {
        id: 100,
        character_body: Some(shape()),
        transform: Transform::new(Vec3::new(1.5, 2., 0.), Vec3::new(1., 1., 1.), 0.).unwrap(),
        ..Default::default()
    });
    World::new(Space::new(Vec3::new(50., 50., 10.)), items)
}

struct Counted<'a> {
    world: &'a World,
    queries: Cell<u64>,
}
impl WorldView for Counted<'_> {
    fn changes(&self) -> Option<&ChangeLog> {
        Some(self.world.changes())
    }
    fn navigation_revision(&self) -> u64 {
        self.world.navigation_revision()
    }
    fn space(&self) -> &Space {
        self.world.space()
    }
    fn items(&self) -> &[Item] {
        self.world.items()
    }
    fn item(&self, id: u64) -> Option<&Item> {
        self.world.item(id)
    }
    fn query(&self, p: Vec3, r: f32) -> Vec<usize> {
        self.queries.set(self.queries.get() + 1);
        self.world.query(p, r)
    }
    fn terrain(&self) -> Option<&HeightField> {
        self.world.terrain()
    }
    fn revision(&self) -> u64 {
        self.world.revision()
    }
    fn spatial_revision(&self) -> u64 {
        self.world.spatial_revision()
    }
    fn physics_stats(&self) -> PhysicsStats {
        self.world.physics_stats()
    }
    fn physics_error(&self) -> Option<&str> {
        self.world.physics_error()
    }
}

#[test]
fn planner_connection_hands_geometry_to_actor_check_in_one_query() {
    use crate::surface::{Action, MovementProfile, SurfaceRoutes};
    use io_traversal::navigation::{Cell as GridCell, Domain, Location, RouteProvider, Stats};
    let mut world = fixture();
    let profile = MovementProfile {
        radius: shape().radius,
        max_slope: shape().max_slope,
        standing_height: shape().height,
        crouch_height: Some(1.1),
        walk_speed: 3.,
        crouch_speed: 1.,
    };
    let mut provider = SurfaceRoutes::new(Domain {
        origin: [0., 0., 0.],
        size: [12., 12.],
        cell_size: 1.,
    })
    .unwrap();
    let from = provider.connectors(&world, profile, Vec3::new(1., 1., 0.), GridCell(1, 1))[0].0;
    let from = Location {
        node: from.node,
        position: from.position,
    };
    let counted = Counted {
        world: &world,
        queries: Cell::new(0),
    };
    let edges = provider.neighbors(&counted, profile, from, &mut Stats::default());
    let edge = edges
        .iter()
        .find(|e| {
            e.action == Action::Walk
                && e.destination.position.x == 2.
                && e.destination.position.y == 1.
        })
        .unwrap_or_else(|| panic!("{edges:?}"));
    counted.queries.set(0);
    assert!(provider.connection_clear(&counted, profile, Some(99), from, edge));
    assert_eq!(
        counted.queries.get(),
        1,
        "only transient actor query remains"
    );
    counted.queries.set(0);
    assert!(provider.live_clear(
        &counted,
        profile,
        Some(99),
        from.position,
        edge.destination.position,
        edge.action
    ));
    assert!(counted.queries.get() > 10, "legacy call retraces scenery");

    assert!(world.set_pose(100, Vec3::new(1.5, 1., 0.), 0.));
    let counted = Counted {
        world: &world,
        queries: Cell::new(0),
    };
    assert!(!provider.connection_clear(&counted, profile, Some(99), from, edge));
    assert_eq!(
        counted.queries.get(),
        1,
        "actor changes do not require tracing unchanged scenery"
    );
    provider.invalidate();
    counted.queries.set(0);
    assert!(!provider.connection_clear(&counted, profile, Some(99), from, edge));
    assert!(
        counted.queries.get() > 1,
        "eviction falls back to complete validation"
    );
}

#[test]
fn cached_scenery_proof_rejects_wrong_world_body_path_or_revision() {
    let mut world = fixture();
    let mut scratch = Vec::new();
    let start = Vec3::default();
    let end = Vec3::new(3., 0., 0.);
    let path = SupportStage
        .run(WalkRequest {
            world: &world,
            actor: None,
            start,
            delta: end,
            shape: shape(),
            scratch: &mut scratch,
        })
        .unwrap();
    let end = path.walk().position;
    let proof = path.scenery_path();
    assert_eq!(
        proof.clear(&world, Some(99), start, end, shape()),
        Some(true)
    );
    assert_eq!(proof.clear(&fixture(), Some(99), start, end, shape()), None);
    assert_eq!(
        proof.clear(&world, Some(99), Vec3::new(0.1, 0., 0.), end, shape()),
        None
    );
    assert_eq!(
        proof.clear(&world, Some(99), start, Vec3::new(4., 0., 0.), shape()),
        None
    );
    assert_eq!(
        proof.clear(
            &world,
            Some(99),
            start,
            end,
            CharacterBody {
                height: 2.5,
                ..shape()
            }
        ),
        None
    );
    let old = world.snapshot();
    world.set_terrain(HeightField::new([-20., -20.], 40., 2, 2, vec![1.; 4]).unwrap());
    assert_eq!(proof.clear(&world, Some(99), start, end, shape()), None);
    assert_eq!(proof.clear(&old, Some(99), start, end, shape()), Some(true));
}

// Retain the exact former double-walk as an independent comparison, not production fallback.
fn reference(world: &dyn WorldView, start: Vec3, end: Vec3, body: CharacterBody) -> bool {
    character_supported_segment(&Scenery(world), None, start, end, body)
        && character_supported_segment(world, Some(99), start, end, body)
}

#[test]
fn actor_stage_reads_the_trajectory_without_resampling_support() {
    let world = fixture();
    let counted = Counted {
        world: &world,
        queries: Cell::new(0),
    };
    let mut scratch = vec![];
    let trajectory = SupportStage
        .run(WalkRequest {
            world: &counted,
            actor: Some(99),
            start: Vec3::default(),
            delta: Vec3::new(3., 0., 0.),
            shape: shape(),
            scratch: &mut scratch,
        })
        .unwrap();
    let points = trajectory.points().as_ptr();
    let support_queries = counted.queries.get();
    assert!(support_queries > 20);
    let validated = ActorClearanceStage.run(trajectory).unwrap();
    assert_eq!(counted.queries.get() - support_queries, 1);
    assert_eq!(
        validated.points().as_ptr(),
        points,
        "no trajectory copy between stages"
    );
    assert!(validated.into_walk().reached);
    counted.queries.set(0);
    assert!(reference(
        &counted,
        Vec3::default(),
        Vec3::new(3., 0., 0.),
        shape()
    ));
    assert!(counted.queries.get() > support_queries + 1);
}

#[test]
fn each_request_observes_current_blockers_body_and_endpoints() {
    let mut world = fixture();
    let start = Vec3::default();
    let end = Vec3::new(3., 0., 0.);
    assert!(segment_clear(&world, Some(99), start, end, shape()));
    world.set_pose(100, Vec3::new(1.5, 0., 0.), 0.);
    assert!(!segment_clear(&world, Some(99), start, end, shape()));
    assert!(!reference(&world, start, end, shape()));
    // Shared actor-less route discovery remains independent of transient bodies.
    assert!(segment_clear(&world, None, start, end, shape()));
    world.set_pose(100, Vec3::new(1.5, 1., 0.), 0.);
    assert!(segment_clear(&world, Some(99), start, end, shape()));
    assert!(!segment_clear(
        &world,
        Some(99),
        start,
        end,
        CharacterBody {
            radius: 0.8,
            ..shape()
        }
    ));
    assert!(!segment_clear(
        &world,
        Some(99),
        start,
        end + Vec3::new(0., 0., 1.),
        shape()
    ));
    assert!(!segment_clear(
        &world,
        Some(99),
        start,
        Vec3::new(f32::NAN, 0., 0.),
        shape()
    ));
    world.set_pose(100, start, 0.);
    assert!(
        !segment_clear(&world, Some(99), start, start, shape()),
        "initial overlap"
    );
}

#[test]
fn staged_trajectory_matches_reference_over_a_crest_and_live_actor_positions() {
    let mut world = fixture();
    world.set_terrain(
        HeightField::new([0., -2.], 2., 5, 3, [0., 1., 2., 1., 0.].repeat(3)).unwrap(),
    );
    let anchor = |w: &World, x| {
        character_support(
            &Scenery(w),
            None,
            Vec3::new(x, 0., 1.),
            shape(),
            SupportProbe::new(3., 3.).unwrap(),
        )
        .unwrap()
        .unwrap()
        .anchor
    };
    let start = anchor(&world, 1.);
    let end = anchor(&world, 7.);
    for x in [1., 2., 3.91, 4., 4.09, 6., 7.] {
        for y in [0., 0.69, 0.71, 2.] {
            world.set_pose(100, Vec3::new(x, y, 2.), 0.);
            for (a, b) in [(start, end), (end, start)] {
                assert_eq!(
                    segment_clear(&world, Some(99), a, b, shape()),
                    reference(&world, a, b, shape()),
                    "{x}/{y}/{a:?}"
                );
            }
        }
    }
}

#[test]
fn plugin_can_interpose_its_own_typed_policy_without_changing_support_or_collision() {
    struct DistancePolicy(f32);
    impl<'a> Stage<SupportedTrajectory<'a>> for DistancePolicy {
        type Output = SupportedTrajectory<'a>;
        type Error = WalkError;
        fn run(&mut self, input: Self::Output) -> Result<Self::Output, Self::Error> {
            let points = input.points();
            let delta = points.last().unwrap().x - points[0].x;
            if delta.abs() > self.0 {
                return Err(WalkError::Invalid("plugin range".into()));
            }
            Ok(input)
        }
    }
    let world = fixture();
    let mut scratch = vec![];
    let mut pipeline = Then(Then(SupportStage, DistancePolicy(1.)), ActorClearanceStage);
    let result = pipeline.run(WalkRequest {
        world: &world,
        actor: Some(99),
        start: Vec3::default(),
        delta: Vec3::new(3., 0., 0.),
        shape: shape(),
        scratch: &mut scratch,
    });
    assert!(matches!(result, Err(WalkError::Invalid(_))));
    assert_eq!(world.item(99).unwrap().transform.anchor, Vec3::default());
}

#[test]
#[ignore = "Release-only scoped timing comparison, not a timing assertion"]
fn movement_stage_cpu_probe() {
    let world = fixture();
    let counted = Counted {
        world: &world,
        queries: Cell::new(0),
    };
    let mut scratch = vec![];
    for pass in [
        "scenery",
        "second-full-walk",
        "new-actor-only",
        "old-total",
        "staged-total",
    ] {
        counted.queries.set(0);
        let mut elapsed = std::time::Duration::ZERO;
        let mut timed_queries = 0;
        for _ in 0..2000 {
            // Set up the actor-only input outside its timed interval.
            let input = if pass == "new-actor-only" {
                Some(
                    SupportStage
                        .run(WalkRequest {
                            world: &counted,
                            actor: Some(99),
                            start: Vec3::default(),
                            delta: Vec3::new(3., 0., 0.),
                            shape: shape(),
                            scratch: &mut scratch,
                        })
                        .unwrap(),
                )
            } else {
                None
            };
            let queries_before = counted.queries.get();
            let started = Instant::now();
            let good = match pass {
                "scenery" => character_supported_segment(
                    &Scenery(&counted),
                    None,
                    Vec3::default(),
                    Vec3::new(3., 0., 0.),
                    shape(),
                ),
                "second-full-walk" => character_supported_segment(
                    &counted,
                    Some(99),
                    Vec3::default(),
                    Vec3::new(3., 0., 0.),
                    shape(),
                ),
                "new-actor-only" => {
                    ActorClearanceStage
                        .run(input.unwrap())
                        .unwrap()
                        .into_walk()
                        .reached
                }
                "old-total" => reference(&counted, Vec3::default(), Vec3::new(3., 0., 0.), shape()),
                _ => {
                    walk(
                        &counted,
                        Some(99),
                        Vec3::default(),
                        Vec3::new(3., 0., 0.),
                        shape(),
                        &mut scratch,
                    )
                    .unwrap()
                    .reached
                }
            };
            elapsed += started.elapsed();
            timed_queries += counted.queries.get() - queries_before;
            assert!(std::hint::black_box(good));
        }
        eprintln!(
            "{pass}: {:.3} us/call; {:.1} queries/call",
            elapsed.as_secs_f64() * 1e6 / 2000.,
            timed_queries as f64 / 2000.
        );
    }
}
