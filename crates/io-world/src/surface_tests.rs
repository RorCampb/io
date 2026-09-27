use crate::*;
use io_types::{Rotation, Vec3};

fn solid(id: u64, anchor: Vec3, shape: ColliderShape) -> Item {
    Item {
        id,
        transform: Transform::new(anchor, Vec3::new(1., 1., 1.), 0.).unwrap(),
        physics_body: Some(PhysicsBody::new(BodyKind::Static)),
        collider: Some(Collider::new(shape)),
        ..Item::default()
    }
}
fn slab(id: u64, top: f32) -> Item {
    solid(
        id,
        Vec3::new(0., 0., top - 0.25),
        ColliderShape::Box {
            half_extents: Vec3::new(4., 4., 0.25),
        },
    )
}
fn world(items: Vec<Item>) -> World {
    World::new(Space::new(Vec3::new(100., 100., 100.)), items)
}
fn column(min_z: f32, max_z: f32) -> SurfaceQuery {
    SurfaceQuery::new(0., 0., min_z, max_z).unwrap()
}
fn near(a: f32, b: f32) {
    assert!((a - b).abs() < 1e-4, "{a} != {b}");
}

#[test]
fn bridge_and_stacked_floors_are_distinct_surfaces_at_the_same_xy() {
    let w = world(vec![slab(9, 0.), slab(2, 4.), slab(7, 8.)]);
    let hits = surface_candidates(&w, column(-1., 10.));
    assert_eq!(
        hits.iter()
            .map(|h| (h.source, h.position.z))
            .collect::<Vec<_>>(),
        vec![
            (SurfaceSource::Collider(7), 8.),
            (SurfaceSource::Collider(2), 4.),
            (SurfaceSource::Collider(9), 0.),
        ]
    );
    assert!(hits.iter().all(|h| h.normal == Vec3::new(0., 0., 1.)));
    assert_eq!(surface_candidates(&w, column(-1., 3.)), vec![hits[2]]);
    assert_eq!(surface_candidates(&w, column(3., 6.)), vec![hits[1]]);
    assert_eq!(
        surface_candidates(&w, column(0., 4.)),
        vec![hits[1], hits[2]]
    );
    assert_eq!(
        surface_candidates(&w, column(-1., 10.).excluding(2)),
        vec![hits[0], hits[2]]
    );
    // Starting in a slab must not fabricate a surface at the query origin or its underside.
    assert_eq!(surface_candidates(&w, column(-1., 3.9)), vec![hits[2]]);
    let short = surface_candidates(&w, column(0., 1e-9));
    assert_eq!(short.len(), 1);
    assert_eq!(short[0].source, hits[2].source);
    near(short[0].position.z, hits[2].position.z);
}

#[test]
fn terrain_and_colliders_share_the_contract_without_a_ground_tag() {
    let mut w = world(vec![slab(1, 4.)]);
    w.set_terrain(HeightField::new([-1., -1.], 2., 2, 2, vec![0.; 4]).unwrap());
    let hits = surface_candidates(&w, column(-1., 6.));
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].source, SurfaceSource::Collider(1));
    assert_eq!(
        hits[1],
        SurfaceHit {
            source: SurfaceSource::Terrain,
            position: Vec3::default(),
            normal: Vec3::new(0., 0., 1.)
        }
    );
    // Lack of terrain coverage is not a world boundary for collider queries.
    assert_eq!(
        surface_candidates(&w, SurfaceQuery::new(3., 0., -1., 6.).unwrap()).len(),
        1
    );
    assert!(surface_candidates(&w, SurfaceQuery::new(10., 0., -1., 6.).unwrap()).is_empty());
}

#[test]
fn terrain_normals_match_each_triangle_and_height_sampling() {
    let t = HeightField::new([0., 0.], 10., 2, 2, vec![0., 10., 20., 40.]).unwrap();
    for (x, y, height, dx, dy) in [
        (5., 2., 11., 1., 3.),
        (2., 5., 14., 2., 2.),
        (5., 5., 20., 1., 3.),
    ] {
        let sample = t.sample(x, y).unwrap();
        assert_eq!(t.height(x, y), Some(sample.height));
        near(sample.height, height);
        near(sample.normal.dot(sample.normal), 1.);
        near(-sample.normal.x / sample.normal.z, dx);
        near(-sample.normal.y / sample.normal.z, dy);
    }
    assert!(t.sample(-0.1, 0.).is_none());
    assert!(t.sample(f32::NAN, 0.).is_none());
}

#[test]
fn oriented_ramp_uses_its_real_face_not_its_axis_aligned_bounds() {
    let angle = 0.4_f32;
    let rotation = Rotation::from_xyzw([0., (angle / 2.).sin(), 0., (angle / 2.).cos()]).unwrap();
    let mut ramp = solid(
        1,
        Vec3::new(0., 0., 2.),
        ColliderShape::Box {
            half_extents: Vec3::new(3., 2., 0.25),
        },
    );
    ramp.transform.rotation = rotation;
    ramp.transform.size = Vec3::new(9., 7., 5.); // Visual scale is not physical scale.
    ramp.collider.as_mut().unwrap().offset = Vec3::new(0.3, 0., 0.2);
    let center = ramp.transform.anchor + rotation.rotate(ramp.collider.unwrap().offset);
    let w = world(vec![ramp]);
    for x in [-1., 0., 1.] {
        let hit = surface_candidates(&w, SurfaceQuery::new(x, 0., -1., 6.).unwrap())[0];
        near(
            hit.position.z,
            center.z + (0.25 - (x - center.x) * angle.sin()) / angle.cos(),
        );
        near(hit.normal.x, angle.sin());
        near(hit.normal.z, angle.cos());
        near(hit.normal.dot(hit.normal), 1.);
        // Existing camera casts and visibility consume the same collider intersection.
        let start = Vec3::new(x, 0., 6.);
        let end = Vec3::new(x, 0., -1.);
        assert!(!sight_segment_clear(&w, start, end).unwrap());
        let sphere = cast_sphere_hit(&w, start, end, 0.01, None)
            .unwrap()
            .unwrap();
        near(sphere.normal.unwrap().z, hit.normal.z);
        near(
            6. - 7. * sphere.fraction,
            hit.position.z + 0.01 / angle.cos(),
        );
    }
}

#[test]
fn spheres_report_curved_normals_and_tiny_spheres_survive_long_queries() {
    let w = world(vec![solid(
        1,
        Vec3::default(),
        ColliderShape::Sphere { radius: 1. },
    )]);
    let hit = surface_candidates(&w, SurfaceQuery::new(0.6, 0., -2., 2.).unwrap())[0];
    near(hit.position.z, 0.8);
    near(hit.normal.x, 0.6);
    near(hit.normal.z, 0.8);
    assert!(surface_candidates(&w, column(-2., 0.5)).is_empty());
    near(surface_candidates(&w, column(-2., 1.))[0].position.z, 1.);
    near(surface_candidates(&w, column(1., 2.))[0].position.z, 1.);
    assert!(surface_candidates(&w, SurfaceQuery::new(1., 0., -2., 2.).unwrap()).is_empty());
    let tiny = world(vec![solid(
        2,
        Vec3::default(),
        ColliderShape::Sphere { radius: 0.01 },
    )]);
    let hit = surface_candidates(&tiny, column(-1., 999.))[0];
    near(hit.position.z, 0.01);
    near(hit.normal.z, 1.);
}

#[test]
fn snapshots_keep_surface_geometry_after_live_item_and_terrain_changes() {
    let mut moving = slab(1, 4.);
    moving.physics_body = Some(PhysicsBody::new(BodyKind::Kinematic));
    let mut w = world(vec![moving]);
    w.set_terrain(HeightField::new([-1., -1.], 2., 2, 2, vec![0.; 4]).unwrap());
    let snapshot = w.snapshot();
    let before = surface_candidates(&snapshot, column(-1., 10.));
    assert!(w.set_kinematic_target(1, Vec3::new(0., 0., 6.75), Rotation::default()));
    w.simulate(&[], 0.1);
    w.set_terrain(HeightField::new([-1., -1.], 2., 2, 2, vec![1.; 4]).unwrap());
    let after = surface_candidates(&w, column(-1., 10.));
    assert_eq!(surface_candidates(&snapshot, column(-1., 10.)), before);
    assert_eq!(
        before.iter().map(|h| h.source).collect::<Vec<_>>(),
        after.iter().map(|h| h.source).collect::<Vec<_>>()
    );
    near(after[0].position.z, 7.);
    near(after[1].position.z, 1.);
    assert_ne!(snapshot.navigation_revision(), w.navigation_revision());
}

#[test]
fn candidates_are_not_character_clearance_or_gameplay_permissions() {
    let w = world(vec![slab(1, 0.), slab(2, 1.)]);
    let hit = surface_candidates(&w, column(-0.1, 0.1))[0];
    let standing = CharacterBody {
        radius: 0.3,
        height: 1.8,
        max_slope: 0.8,
    };
    assert_eq!(hit.source, SurfaceSource::Collider(1));
    assert!(!character_space_fits(&w, hit.position, standing));
    assert!(character_space_fits(
        &w,
        hit.position,
        CharacterBody {
            height: 0.5,
            ..standing
        }
    ));
    // Occupancy and a character's body do not silently become collision surfaces.
    let empty = world(vec![
        Item {
            id: 1,
            character_body: Some(standing),
            ..Item::default()
        },
        Item {
            id: 2,
            ..Item::default()
        },
    ]);
    assert!(surface_candidates(&empty, column(-1., 5.)).is_empty());
}

#[test]
fn query_validation_and_equal_height_ordering_are_deterministic() {
    assert_eq!(
        SurfaceQuery::new(f32::NAN, 0., 0., 1.).unwrap_err(),
        SurfaceQueryError::NonFinite
    );
    assert_eq!(
        SurfaceQuery::new(0., f32::INFINITY, 0., 1.).unwrap_err(),
        SurfaceQueryError::NonFinite
    );
    assert_eq!(
        SurfaceQuery::new(1_000_001., 0., 0., 1.).unwrap_err(),
        SurfaceQueryError::CoordinatesOutOfRange
    );
    for (min, max) in [(1., 1.), (2., 1.), (0., 1001.)] {
        assert_eq!(
            SurfaceQuery::new(0., 0., min, max).unwrap_err(),
            SurfaceQueryError::InvalidVerticalRange
        );
    }
    let mut a = world(vec![slab(3, 0.), slab(2, 0.)]);
    let mut b = world(vec![slab(2, 0.), slab(3, 0.)]);
    for w in [&mut a, &mut b] {
        w.set_terrain(HeightField::new([-1., -1.], 2., 2, 2, vec![0.; 4]).unwrap());
    }
    let hits = surface_candidates(&a, column(-1., 1.));
    assert_eq!(hits, surface_candidates(&b, column(-1., 1.)));
    assert_eq!(
        hits.iter().map(|h| h.source).collect::<Vec<_>>(),
        vec![
            SurfaceSource::Terrain,
            SurfaceSource::Collider(2),
            SurfaceSource::Collider(3)
        ]
    );
}
