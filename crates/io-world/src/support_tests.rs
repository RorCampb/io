use crate::*;
use io_types::{Rotation, Vec3};

fn body() -> CharacterBody {
    CharacterBody {
        radius: 0.3,
        height: 1.8,
        max_slope: 0.8,
    }
}
fn block(id: u64, center: Vec3, half: Vec3, rotation: Rotation) -> Item {
    Item {
        id,
        transform: Transform::oriented(center, Vec3::new(1., 1., 1.), rotation).unwrap(),
        collider: Some(Collider::new(ColliderShape::Box { half_extents: half })),
        physics_body: Some(PhysicsBody::new(BodyKind::Static)),
        ..Item::default()
    }
}
fn floor(id: u64, z: f32) -> Item {
    block(
        id,
        Vec3::new(0., 0., z - 0.25),
        Vec3::new(8., 8., 0.25),
        Rotation::default(),
    )
}
fn world(items: Vec<Item>) -> World {
    World::new(Space::new(Vec3::new(100., 100., 100.)), items)
}
fn support(w: &dyn WorldView, p: Vec3, shape: CharacterBody) -> Option<CharacterSupport> {
    character_support(w, None, p, shape, SupportProbe::CONTACT).unwrap()
}
fn near(a: f32, b: f32) {
    assert!((a - b).abs() < 1e-4, "{a} != {b}");
}
fn ramp() -> World {
    let angle = 0.3_f32;
    world(vec![block(
        1,
        Vec3::new(0., 0., 2.),
        Vec3::new(5., 3., 0.25),
        Rotation::from_xyzw([0., (angle * 0.5).sin(), 0., (angle * 0.5).cos()]).unwrap(),
    )])
}
fn ramp_start(w: &World) -> Vec3 {
    character_support(
        w,
        None,
        Vec3::new(-2., 0., 2.),
        body(),
        SupportProbe::new(4., 4.).unwrap(),
    )
    .unwrap()
    .unwrap()
    .anchor
}

#[test]
fn support_selects_the_local_floor_checks_headroom_and_never_uses_actors() {
    let w = world(vec![floor(1, 0.), floor(2, 4.), floor(3, 5.3)]);
    assert_eq!(
        support(&w, Vec3::default(), body()).unwrap().surface.source,
        SurfaceSource::Collider(1)
    );
    assert!(support(&w, Vec3::new(0., 0., 4.), body()).is_none());
    let short = CharacterBody {
        height: 0.7,
        ..body()
    };
    assert_eq!(
        support(&w, Vec3::new(0., 0., 4.), short)
            .unwrap()
            .surface
            .source,
        SurfaceSource::Collider(2)
    );
    assert!(support(&w, Vec3::new(0., 0., 2.), body()).is_none());
    assert!(
        character_support(&w, Some(1), Vec3::default(), body(), SupportProbe::CONTACT)
            .unwrap()
            .is_none()
    );
    let w = world(vec![Item {
        id: 1,
        character_body: Some(body()),
        ..Item::default()
    }]);
    assert!(support(&w, Vec3::new(0., 0., 1.8), body()).is_none());
}

#[test]
fn oriented_clearance_does_not_turn_rotated_walls_into_large_solid_rectangles() {
    let w = world(vec![block(
        1,
        Vec3::new(0., 0., 1.),
        Vec3::new(2., 0.05, 1.),
        Rotation::yaw(std::f32::consts::FRAC_PI_4).unwrap(),
    )]);
    let a = Vec3::new(-1., 1., 0.);
    let b = Vec3::new(1., -1., 0.);
    assert!(character_space_fits(&w, a, body()));
    assert!(character_space_fits(&w, b, body()));
    assert!(!character_colliders_clear(&w, None, a, b, body()));
    let stop = sweep_character(&w, 99, a, b - a, body()).unwrap();
    assert!(character_space_fits(&w, stop.position, body()));
    assert!(!stop.grounded, "a wall is not a floor");
}

#[test]
fn motor_queries_walk_up_and_down_real_ramps_with_the_same_slope_limit() {
    let w = ramp();
    let start = ramp_start(&w);
    near(start.z, 2. + 0.25 / 0.3_f32.cos() + 2.3 * 0.3_f32.tan());
    assert!(character_space_fits(&w, start, body()));
    let down = walk_character(&w, None, start, Vec3::new(4., 0., 0.), body()).unwrap();
    assert!(down.reached, "{down:?}");
    near(down.position.z, start.z - 4. * 0.3_f32.tan());
    assert!(character_supported_segment(
        &w,
        None,
        start,
        down.position,
        body()
    ));
    let up = walk_character(&w, None, down.position, Vec3::new(-4., 0., 0.), body()).unwrap();
    assert!(up.reached, "{up:?}");
    near(up.position.z, start.z);
    let limited = CharacterBody {
        max_slope: 0.1,
        ..body()
    };
    assert!(support(&w, start, limited).is_none());
    assert!(
        !walk_character(&w, None, start, Vec3::new(1., 0., 0.), limited)
            .unwrap()
            .reached
    );
    assert!(!character_supported_segment(
        &w,
        None,
        start,
        down.position,
        limited
    ));
    let landed = sweep_character(
        &w,
        99,
        start + Vec3::new(0., 0., 3.),
        Vec3::new(0., 0., -4.),
        body(),
    )
    .unwrap();
    assert!(landed.grounded, "{landed:?}");
    near(landed.position.z, start.z);
}

#[test]
fn terrain_uses_the_whole_footprint_instead_of_sinking_its_uphill_corner() {
    let mut w = world(vec![]);
    w.set_terrain(HeightField::new([-5., -5.], 10., 2, 2, vec![0., 2., 0., 2.]).unwrap());
    let start = character_support(
        &w,
        None,
        Vec3::default(),
        body(),
        SupportProbe::new(4., 4.).unwrap(),
    )
    .unwrap()
    .unwrap()
    .anchor;
    near(start.z, 1.06);
    let up = walk_character(&w, None, start, Vec3::new(2., 0., 0.), body()).unwrap();
    assert!(up.reached, "{up:?}");
    near(up.position.z, 1.46);
    assert!(character_supported_segment(
        &w,
        None,
        start,
        up.position,
        body()
    ));
    let sample = w.terrain().unwrap().height(start.x, start.y).unwrap();
    assert!(!character_space_fits(
        &w,
        Vec3::new(start.x, start.y, sample),
        body()
    ));

    w.set_terrain(
        HeightField::new(
            [-1., -1.],
            1.,
            3,
            3,
            vec![0., 0., 0., 0., 1., 0., 0., 0., 0.],
        )
        .unwrap(),
    );
    let wide = CharacterBody {
        radius: 1.,
        max_slope: 2.,
        ..body()
    };
    // All four footprint corners are zero, but a peak inside the footprint is one.
    assert!(!character_space_fits(&w, Vec3::new(0., 0., 0.5), wide));
    near(
        character_support(&w, None, Vec3::new(0., 0., 1.), wide, SupportProbe::CONTACT)
            .unwrap()
            .unwrap()
            .anchor
            .z,
        1.,
    );
}

#[test]
fn gaps_and_vertical_steps_are_not_implicit_jump_or_climb_abilities() {
    let w = world(vec![
        block(
            1,
            Vec3::new(-2., 0., -0.25),
            Vec3::new(1.9, 2., 0.25),
            Rotation::default(),
        ),
        block(
            2,
            Vec3::new(2., 0., -0.25),
            Vec3::new(1.9, 2., 0.25),
            Rotation::default(),
        ),
    ]);
    let a = Vec3::new(-1., 0., 0.);
    let b = Vec3::new(1., 0., 0.);
    assert!(!character_supported_segment(&w, None, a, b, body()));
    assert!(!walk_character(&w, None, a, b - a, body()).unwrap().reached);
    let w = world(vec![
        floor(1, 0.),
        block(
            2,
            Vec3::new(1., 0., 0.25),
            Vec3::new(1., 2., 0.25),
            Rotation::default(),
        ),
    ]);
    let walk = walk_character(&w, None, a, Vec3::new(2., 0., 0.), body()).unwrap();
    assert!(!walk.reached);
    assert!(walk.position.x < -0.29);
}

#[test]
fn live_support_can_disappear_without_changing_snapshot_answers() {
    let mut platform = floor(1, 0.);
    platform.physics_body = Some(PhysicsBody::new(BodyKind::Kinematic));
    let mut w = world(vec![platform]);
    let snapshot = w.snapshot();
    assert!(w.set_kinematic_target(1, Vec3::new(30., 0., -0.25), Rotation::default()));
    w.simulate(&[], 0.1);
    assert!(support(&snapshot, Vec3::default(), body()).is_some());
    assert!(support(&w, Vec3::default(), body()).is_none());
    assert!(!character_supported_segment(
        &w,
        None,
        Vec3::default(),
        Vec3::new(1., 0., 0.),
        body()
    ));
}

#[test]
fn invalid_queries_fail_before_work_and_missing_terrain_does_not_block_a_bridge() {
    for (above, below) in [(0., 0.), (-1., 1.), (f32::NAN, 1.), (1., 5.)] {
        assert!(SupportProbe::new(above, below).is_err());
    }
    let mut w = world(vec![floor(1, 0.)]);
    w.set_terrain(HeightField::new([20., 20.], 1., 2, 2, vec![0.; 4]).unwrap());
    assert!(support(&w, Vec3::default(), body()).is_some());
    assert!(
        walk_character(&w, None, Vec3::default(), Vec3::new(1., 0., 0.), body())
            .unwrap()
            .reached
    );
    for delta in [
        Vec3::new(0., 0., 1.),
        Vec3::new(f32::NAN, 0., 0.),
        Vec3::new(129., 0., 0.),
    ] {
        assert!(walk_character(&w, None, Vec3::default(), delta, body()).is_err());
    }
    assert!(character_support(
        &w,
        None,
        Vec3::default(),
        CharacterBody {
            radius: 0.,
            ..body()
        },
        SupportProbe::CONTACT
    )
    .is_err());
}

#[test]
fn continuous_terrain_clearance_catches_a_ridge_between_clear_endpoints() {
    let mut w = world(vec![]);
    w.set_terrain(
        HeightField::new(
            [0., -1.],
            1.,
            5,
            3,
            vec![0., 0., 2., 0., 0., 0., 0., 2., 0., 0., 0., 0., 2., 0., 0.],
        )
        .unwrap(),
    );
    let a = Vec3::new(0.5, 0., 0.5);
    let b = Vec3::new(3.5, 0., 0.5);
    assert!(character_space_fits(&w, a, body()));
    assert!(character_space_fits(&w, b, body()));
    assert!(!character_segment_clear(&w, None, a, b, body()));
    let above = Vec3::new(0., 0., 2.);
    assert!(character_segment_clear(
        &w,
        None,
        a + above,
        b + above,
        body()
    ));
    let falling = sweep_character(
        &w,
        99,
        Vec3::new(2., 0., 4.),
        Vec3::new(0., 0., -5.),
        body(),
    )
    .unwrap();
    assert!(falling.position.z >= 2. - 1e-4);
    assert!(character_space_fits(&w, falling.position, body()));
}

#[test]
fn wall_contact_and_sphere_envelopes_cannot_manufacture_walkable_support() {
    let w = world(vec![block(
        1,
        Vec3::new(0., 0., 2.),
        Vec3::new(0.1, 2., 2.),
        Rotation::default(),
    )]);
    assert!(support(&w, Vec3::new(0.40001, 0., 1.), body()).is_none());
    let w = world(vec![Item {
        id: 1,
        collider: Some(Collider::new(ColliderShape::Sphere { radius: 1. })),
        physics_body: Some(PhysicsBody::new(BodyKind::Static)),
        ..Item::default()
    }]);
    assert!(support(&w, Vec3::new(0., 0., 1.), body()).is_none());
    assert!(!character_space_fits(&w, Vec3::default(), body()));
}

#[test]
fn slope_transitions_cross_a_terrain_crest_without_tunnelling_or_getting_stuck() {
    let mut w = world(vec![]);
    w.set_terrain(
        HeightField::new(
            [0., -2.],
            2.,
            3,
            3,
            vec![0., 1., 0., 0., 1., 0., 0., 1., 0.],
        )
        .unwrap(),
    );
    let start = character_support(
        &w,
        None,
        Vec3::new(0.4, 0., 0.),
        body(),
        SupportProbe::new(2., 2.).unwrap(),
    )
    .unwrap()
    .unwrap()
    .anchor;
    let walk = walk_character(&w, None, start, Vec3::new(3.2, 0., 0.), body()).unwrap();
    assert!(walk.reached, "{walk:?}");
    near(walk.position.z, start.z);
    assert!(character_supported_segment(
        &w,
        None,
        start,
        walk.position,
        body()
    ));
}

#[test]
fn traced_walk_retains_safe_segments_and_reuses_storage_without_old_results() {
    let mut w = world(vec![]);
    w.set_terrain(
        HeightField::new(
            [0., -2.],
            2.,
            3,
            3,
            vec![0., 1., 0., 0., 1., 0., 0., 1., 0.],
        )
        .unwrap(),
    );
    let start = character_support(
        &w,
        None,
        Vec3::new(0.4, 0., 0.),
        body(),
        SupportProbe::new(2., 2.).unwrap(),
    )
    .unwrap()
    .unwrap()
    .anchor;
    let mut points = Vec::with_capacity(256);
    let walk =
        trace_character_walk(&w, None, start, Vec3::new(3.2, 0., 0.), body(), &mut points).unwrap();
    assert!(walk.reached);
    assert_eq!(points[0], start);
    assert_eq!(*points.last().unwrap(), walk.position);
    assert!(points.iter().any(|p| p.z > 0.99));
    for segment in points.windows(2) {
        assert!(character_segment_clear(
            &w,
            None,
            segment[0],
            segment[1],
            body()
        ));
    }
    let ordinary = walk_character(&w, None, start, Vec3::new(3.2, 0., 0.), body()).unwrap();
    assert_eq!(walk.position, ordinary.position);
    let capacity = points.capacity();
    let short = trace_character_walk(
        &w,
        None,
        start,
        Vec3::new(0.01, 0., 0.),
        body(),
        &mut points,
    )
    .unwrap();
    assert!(short.reached && points.len() == 2);
    assert_eq!(points.capacity(), capacity);
    assert!(trace_character_walk(
        &w,
        None,
        start,
        Vec3::new(f32::NAN, 0., 0.),
        body(),
        &mut points
    )
    .is_err());
    assert!(points.is_empty());
    let empty = world(vec![]);
    assert!(
        !trace_character_walk(&empty, None, start, Vec3::default(), body(), &mut points)
            .unwrap()
            .reached
    );
    assert!(points.is_empty());
}

#[test]
fn failed_trace_stops_at_the_accepted_prefix_before_a_wall() {
    let w = world(vec![
        floor(1, 0.),
        block(
            2,
            Vec3::new(1., 0., 1.),
            Vec3::new(0.01, 1., 1.),
            Rotation::default(),
        ),
    ]);
    let mut points = vec![];
    let walk = trace_character_walk(
        &w,
        None,
        Vec3::default(),
        Vec3::new(2., 0., 0.),
        body(),
        &mut points,
    )
    .unwrap();
    assert!(!walk.reached);
    assert_eq!(*points.last().unwrap(), walk.position);
    assert!(points.iter().all(|p| p.x < 0.71));
    assert!(points
        .windows(2)
        .all(|p| character_segment_clear(&w, None, p[0], p[1], body())));
}
