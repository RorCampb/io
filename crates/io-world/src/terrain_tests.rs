//! Terrain, character-body contracts and visibility regression tests.
use crate::*;
use io_types::Vec3;

#[test]
fn height_sampling_matches_triangles_and_rejects_bad_contracts() {
    let t = HeightField::new([0., 0.], 10., 2, 2, vec![0., 10., 20., 40.]).unwrap();
    assert_eq!(t.height(5., 2.), Some(11.));
    assert_eq!(t.height(2., 5.), Some(14.));
    assert_eq!(t.height(10., 10.), Some(40.));
    assert_eq!(t.height(-1., 0.), None);
    assert_eq!(t.height(f32::NAN, 0.), None);
    assert!(HeightField::new([0., 0.], 1., 2, 2, vec![0.; 3]).is_err());
    assert!(HeightField::new([0., 0.], 1., 2, 2, vec![f32::NAN; 4]).is_err());
}
fn walker() -> Item {
    Item {
        id: 1,
        character_body: Some(CharacterBody {
            radius: 0.3,
            height: 1.8,
            max_slope: 1.,
        }),
        ..Default::default()
    }
}

#[test]
fn sight_checks_thin_rotated_colliders_and_exact_terrain_ridges() {
    let mut a = walker();
    a.transform.anchor = Vec3::new(-4., 0., 0.);
    let mut b = walker();
    b.id = 2;
    b.transform.anchor = Vec3::new(4., 0., 0.);
    let wall = Item {
        id: 3,
        physics_body: Some(PhysicsBody::new(BodyKind::Static)),
        collider: Some(Collider::new(ColliderShape::Box {
            half_extents: Vec3::new(0.01, 2., 2.),
        })),
        transform: Transform::new(Vec3::default(), Vec3::new(1., 1., 1.), 0.4).unwrap(),
        ..Default::default()
    };
    let mut world = World::new(Space::new(Vec3::new(100., 100., 100.)), vec![a, b, wall]);
    assert!(!line_of_sight(&world, 1, 2).unwrap());
    world.set_pose(2, Vec3::new(-4., 4., 0.), 0.);
    assert!(line_of_sight(&world, 1, 2).unwrap());
    assert!(line_of_sight(&world, 1, 999).is_err());
    let terrain = HeightField::new([0., 0.], 1., 3, 2, vec![0., 5., 0., 0., 5., 0.]).unwrap();
    assert!(terrain.occludes(Vec3::new(0., 0.4, 2.), Vec3::new(2., 0.4, 2.)));
    assert!(!terrain.occludes(Vec3::new(0., 0.4, 6.), Vec3::new(2., 0.4, 6.)));
    // Height maximum occurs at the internal diagonal, not an X/Y grid edge.
    let diagonal = HeightField::new([0., 0.], 1., 2, 2, vec![5., 0., 0., 5.]).unwrap();
    assert!(diagonal.occludes(Vec3::new(0., 1., 4.), Vec3::new(1., 0., 4.)));
}
#[test]
fn walkers_follow_hills_and_cannot_tunnel_through_thin_walls() {
    let wall = Item {
        id: 2,
        physics_body: Some(PhysicsBody::new(BodyKind::Static)),
        collider: Some(Collider::new(ColliderShape::Box {
            half_extents: Vec3::new(0.05, 10., 3.),
        })),
        transform: Transform::new(Vec3::new(2., 0., 0.), Vec3::new(1., 1., 1.), 0.).unwrap(),
        ..Default::default()
    };
    let mut world = World::new(
        Space::new(Vec3::new(100., 100., 100.)),
        vec![walker(), wall],
    );
    world.set_terrain(
        HeightField::new(
            [-10., -10.],
            10.,
            3,
            3,
            vec![0., 1., 2., 0., 1., 2., 0., 1., 2.],
        )
        .unwrap(),
    );
    world.set_pose(1, Vec3::new(0., 0., 1.), 0.);
    let end = terrain_destination(&world, 1, Vec3::new(10., 0., 0.)).unwrap();
    assert!(end.x > 1.4 && end.x < 1.66, "{end:?}");
    assert!((end.z - (1. + end.x * 0.1)).abs() < 1e-5);
    let slide = terrain_destination(&world, 1, Vec3::new(5., 5., 0.)).unwrap();
    assert!(slide.y > 4.8 && slide.x < 1.66);
    let snapshot = world.snapshot();
    assert!(std::ptr::eq(
        world.terrain().unwrap(),
        snapshot.terrain().unwrap()
    ));
    assert_eq!(snapshot.terrain().unwrap().height(0., 0.), Some(1.));
}
#[test]
fn character_body_ownership_and_death_handoff_are_checked() {
    let mut actor = walker();
    actor.physics_body = Some(PhysicsBody::new(BodyKind::Dynamic));
    assert!(actor.validate().is_err());
    let mut world = World::new(Space::new(Vec3::new(100., 100., 100.)), vec![walker()]);
    world
        .attach_dynamic_body(
            1,
            PhysicsBody::new(BodyKind::Dynamic),
            Collider::new(ColliderShape::Sphere { radius: 0.3 }),
            Vec3::new(1., 0., 1.),
            Vec3::default(),
        )
        .unwrap();
    assert!(world.item(1).unwrap().character_body.is_none());
    assert!(world.item(1).unwrap().physics_body.is_some());
}

#[test]
fn character_body_is_present_in_air_and_grounded_is_only_a_support_result() {
    let mut actor = walker();
    actor.transform.anchor = Vec3::new(0., 0., 3.);
    let body = actor.character_body.unwrap();
    let floor = Item {
        id: 2,
        transform: Transform::new(Vec3::new(0., 0., -0.5), Vec3::new(10., 10., 1.), 0.).unwrap(),
        collider: Some(Collider::new(ColliderShape::Box {
            half_extents: Vec3::new(5., 5., 0.5),
        })),
        physics_body: Some(PhysicsBody::new(BodyKind::Static)),
        ..Default::default()
    };
    let mut world = World::new(Space::new(Vec3::new(20., 20., 20.)), vec![actor, floor]);
    let airborne =
        sweep_character(&world, 1, Vec3::new(0., 0., 3.), Vec3::default(), body).unwrap();
    assert!(!airborne.grounded);
    assert_eq!(world.item(1).unwrap().character_body, Some(body));
    assert!(world.set_pose(1, Vec3::default(), 0.));
    let supported = sweep_character(&world, 1, Vec3::default(), Vec3::default(), body).unwrap();
    assert!(supported.grounded);
    let bounds = body.bounds_at(Vec3::new(1., 2., 3.));
    assert_eq!(bounds.min, Vec3::new(0.7, 1.7, 3.));
    assert_eq!(bounds.max, Vec3::new(1.3, 2.3, 4.8));
}
