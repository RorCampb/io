use super::*;
use crate::{
    BodyKind, Collider, ColliderShape, Item, PhysicsBody, Space, Transform, World, WorldView,
};
use io_types::{Bounds, Rotation};
fn layout() -> (Vec<Interior>, Vec<Portal>) {
    let regions = (0..2)
        .map(|i| Interior {
            name: format!("Room {i}"),
            bounds: Bounds {
                min: Vec3::new(-2., i as f32 * 4., 0.),
                max: Vec3::new(2., (i + 1) as f32 * 4., 3.),
            },
            ceiling: Some(3.),
            entry_direction: Vec3::new(0., 1., 0.),
        })
        .collect();
    let portals = (0..2)
        .map(|i| Portal {
            name: format!("Door {i}"),
            from: if i == 0 {
                SpaceLocation::Exterior
            } else {
                SpaceLocation::Interior(InteriorId(0))
            },
            to: SpaceLocation::Interior(InteriorId(i)),
            center: Vec3::new(0., i as f32 * 4., 1.),
            normal: Vec3::new(0., 1., 0.),
            width: 2.,
            height: 2.,
        })
        .collect();
    (regions, portals)
}
fn point(y: f32) -> Vec3 {
    Vec3::new(0., y, 0.)
}
#[test]
fn crossing_requires_an_opening_not_proximity_or_a_wall() {
    let (r, p) = layout();
    validate_layout(&r, &p).unwrap();
    assert_eq!(
        advance_location(&p, SpaceLocation::Exterior, point(-1.), point(1.)),
        SpaceLocation::Interior(InteriorId(0))
    );
    assert_eq!(
        advance_location(
            &p,
            SpaceLocation::Interior(InteriorId(0)),
            point(1.),
            point(-1.)
        ),
        SpaceLocation::Exterior
    );
    for (a, b) in [
        (Vec3::new(1.5, -1., 0.), Vec3::new(1.5, 1., 0.)),
        (Vec3::new(-3., 2., 0.), Vec3::new(0., 2., 0.)),
        (Vec3::new(0., -1., 2.5), Vec3::new(0., 1., 2.5)),
    ] {
        assert_eq!(
            advance_location(&p, SpaceLocation::Exterior, a, b),
            SpaceLocation::Exterior
        );
    }
    assert!(p[0]
        .approach(SpaceLocation::Exterior, point(-1.), 3.)
        .is_some());
    assert!(p[0]
        .approach(SpaceLocation::Exterior, Vec3::new(1.5, -1., 0.), 3.)
        .is_none());
    assert!(p[0]
        .approach(SpaceLocation::Exterior, point(1.), 3.)
        .is_none());
}
#[test]
fn exact_plane_stops_and_fast_multi_room_moves_are_directional() {
    let (_, mut p) = layout();
    p.reverse();
    assert_eq!(
        advance_location(&p, SpaceLocation::Exterior, point(-1.), point(6.)),
        SpaceLocation::Interior(InteriorId(1))
    );
    assert_eq!(
        advance_location(
            &p,
            SpaceLocation::Interior(InteriorId(1)),
            point(6.),
            point(-1.)
        ),
        SpaceLocation::Exterior
    );
    let on = advance_location(&p, SpaceLocation::Exterior, point(-1.), point(0.));
    assert_eq!(on, SpaceLocation::Exterior);
    assert_eq!(
        advance_location(&p, on, point(0.), point(1.)),
        SpaceLocation::Interior(InteriorId(0))
    );
}
#[test]
fn membership_uses_existing_mutations_and_snapshots_and_teleports_reclassify() {
    let (r, p) = layout();
    let item = Item {
        id: 7,
        transform: Transform::new(point(-1.), Vec3::new(1., 1., 1.), 0.).unwrap(),
        ..Default::default()
    };
    let mut w = World::new(Space::new(Vec3::new(100., 100., 100.)), vec![item]);
    w.set_space_layout(r, p).unwrap();
    let before = w.snapshot();
    w.set_pose(7, point(5.), 0.);
    assert_eq!(
        w.space_location(7),
        Some(SpaceLocation::Interior(InteriorId(1)))
    );
    assert_eq!(before.space_location(7), Some(SpaceLocation::Exterior));
    assert_eq!(before.portals().len(), 2);
    w.teleport_pose(7, point(-2.), Rotation::yaw(0.).unwrap());
    assert_eq!(w.space_location(7), Some(SpaceLocation::Exterior));
    let revision = w.revision();
    assert!(!w.teleport_pose(7, Vec3::new(f32::NAN, 0., 0.), Rotation::yaw(0.).unwrap()));
    assert_eq!(w.revision(), revision);
    w.teleport_pose(7, point(2.), Rotation::yaw(0.).unwrap());
    assert_eq!(
        w.space_location(7),
        Some(SpaceLocation::Interior(InteriorId(0)))
    );
    assert_eq!(w.space_location(99), None);
}
#[test]
fn physics_updates_membership_without_visibility_or_a_camera() {
    let (r, p) = layout();
    let mut body = PhysicsBody::new(BodyKind::Dynamic);
    body.gravity_scale = 0.;
    body.velocity = Vec3::new(0., 4., 0.);
    let item = Item {
        id: 1,
        transform: Transform::new(Vec3::new(0., -1., 1.), Vec3::new(1., 1., 1.), 0.).unwrap(),
        physics_body: Some(body),
        collider: Some(Collider::new(ColliderShape::Sphere { radius: 0.1 })),
        ..Default::default()
    };
    let mut w = World::new(Space::new(Vec3::new(100., 100., 100.)), vec![item]);
    w.set_space_layout(r, p).unwrap();
    for _ in 0..30 {
        w.simulate(&[], 1. / 60.);
    }
    assert_eq!(
        w.space_location(1),
        Some(SpaceLocation::Interior(InteriorId(0)))
    );
    assert!(w.physics_error().is_none());
}
#[test]
fn invalid_portal_geometry_and_layout_replacement_are_rejected() {
    let (r, p) = layout();
    for bad in [
        Portal {
            width: 9.,
            ..p[0].clone()
        },
        Portal {
            normal: Vec3::new(0., -1., 0.),
            ..p[0].clone()
        },
        Portal {
            to: SpaceLocation::Interior(InteriorId(99)),
            ..p[0].clone()
        },
        Portal {
            center: Vec3::new(0., 1., 1.),
            ..p[0].clone()
        },
    ] {
        assert!(bad.validate(&r).is_err());
    }
    let mut w = World::new(Space::new(Vec3::new(10., 10., 10.)), vec![]);
    w.set_space_layout(r.clone(), p.clone()).unwrap();
    let revision = w.revision();
    assert!(w
        .set_space_layout(r, vec![p[0].clone(), p[0].clone()])
        .is_err());
    assert_eq!(w.revision(), revision);
    assert_eq!(w.portals().len(), 2);
}
#[test]
fn spawn_membership_respects_floors_and_does_not_claim_rooftops() {
    let (mut r, _) = layout();
    r[1].bounds = Bounds {
        min: Vec3::new(-2., 0., 3.),
        max: Vec3::new(2., 4., 6.),
    };
    r[1].ceiling = Some(6.);
    assert_eq!(
        locate_space(&r, Vec3::new(0., 2., 3.)),
        SpaceLocation::Interior(InteriorId(1))
    );
    assert_eq!(
        locate_space(&r, Vec3::new(0., 2., 6.)),
        SpaceLocation::Exterior
    );
}
