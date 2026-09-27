//! Read-only conservative sweeps for presentation and gameplay queries.
use crate::{geometry::segment_collider, WorldView};
use io_types::Vec3;

#[derive(Clone, Copy, Debug)]
pub struct SphereCastHit {
    pub fraction: f32,
    /// Outward world-space normal. Absent for initial overlaps or conservative terrain hits.
    pub normal: Option<Vec3>,
}

/// First fraction of travel blocked by a collider expanded by `radius`.
/// Oriented boxes retain their orientation; their rounded corners are conservatively
/// boxed. The excluded item is normally the camera's followed actor.
/// Heightfield clearance is conservative using its maximum triangle slope.
pub fn cast_sphere(
    world: &dyn WorldView,
    start: Vec3,
    end: Vec3,
    radius: f32,
    exclude: Option<u64>,
) -> Result<f32, &'static str> {
    cast_sphere_hit(world, start, end, radius, exclude).map(|hit| hit.map_or(1., |h| h.fraction))
}

pub fn cast_sphere_hit(
    world: &dyn WorldView,
    start: Vec3,
    end: Vec3,
    radius: f32,
    exclude: Option<u64>,
) -> Result<Option<SphereCastHit>, &'static str> {
    let delta = end - start;
    let length = delta.dot(delta).sqrt();
    if !start.finite()
        || !end.finite()
        || !length.is_finite()
        || length > 1000.
        || !radius.is_finite()
        || !(0.001..=1000.).contains(&radius)
    {
        return Err("invalid sphere sweep");
    }
    let terrain = world
        .terrain()
        .map_or(1., |t| t.sphere_fraction(start, end, radius));
    let mut hit = (terrain < 1.).then_some(SphereCastHit {
        fraction: terrain,
        normal: None,
    });
    for i in world.query(start + delta.scaled(0.5), length * 0.5 + radius) {
        let item = &world.items()[i];
        if Some(item.id) == exclude {
            continue;
        }
        let Some(collider) = &item.collider else {
            continue;
        };
        if let Some(contact) = segment_collider(&item.transform, collider, start, delta, radius) {
            if hit.is_none_or(|h| contact.fraction < h.fraction as f64) {
                hit = Some(SphereCastHit {
                    fraction: contact.fraction as f32,
                    normal: contact.normal,
                });
            }
        }
    }
    Ok(hit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BodyKind, Collider, ColliderShape, Item, PhysicsBody, Space, Transform, World};
    #[test]
    fn terrain_sphere_stops_above_ground_and_detects_intervening_ridges() {
        let flat = crate::HeightField::new([0., 0.], 1., 3, 3, vec![0.; 9]).unwrap();
        let start = Vec3::new(1., 1., 2.);
        let end = Vec3::new(1., 1., -2.);
        let t = flat.sphere_fraction(start, end, 0.2);
        assert!((t - 0.44975).abs() < 1e-5);
        let ridge =
            crate::HeightField::new([0., 0.], 1., 3, 2, vec![0., 1., 0., 0., 1., 0.]).unwrap();
        assert!(ridge.sphere_fraction(Vec3::new(0., 0.5, 0.8), Vec3::new(2., 0.5, 0.8), 0.1) < 0.5);
    }
    #[test]
    fn offset_collider_is_indexed_even_without_a_matching_visual() {
        let mut c = Collider::new(ColliderShape::Box {
            half_extents: Vec3::new(1., 1., 1.),
        });
        c.offset = Vec3::new(100., 0., 0.);
        let mut w = World::new(
            Space::new(Vec3::new(1000., 1000., 1000.)),
            vec![Item {
                id: 1,
                collider: Some(c),
                physics_body: Some(PhysicsBody::new(BodyKind::Kinematic)),
                ..Item::default()
            }],
        );
        let start = Vec3::new(95., 0., 0.);
        let end = Vec3::new(105., 0., 0.);
        assert!(cast_sphere(&w, start, end, 0.2, None).unwrap() < 1.);
        let snapshot = w.snapshot();
        assert!(w.set_kinematic_target(1, Vec3::new(100., 0., 0.), io_types::Rotation::default()));
        w.simulate(&[], 0.1);
        assert_eq!(cast_sphere(&w, start, end, 0.2, None).unwrap(), 1.);
        assert!(cast_sphere(&snapshot, start, end, 0.2, None).unwrap() < 1.);
        assert!(
            cast_sphere(
                &w,
                start + Vec3::new(100., 0., 0.),
                end + Vec3::new(100., 0., 0.),
                0.2,
                None
            )
            .unwrap()
                < 1.
        );
    }
    fn world(shape: ColliderShape, yaw: f32) -> World {
        World::new(
            Space::new(Vec3::new(100., 100., 100.)),
            vec![Item {
                id: 1,
                transform: Transform::new(Vec3::new(5., 0., 0.), Vec3::new(1., 1., 1.), yaw)
                    .unwrap(),
                physics_body: Some(PhysicsBody::new(BodyKind::Static)),
                collider: Some(Collider::new(shape)),
                ..Item::default()
            }],
        )
    }
    #[test]
    fn thin_rotated_boxes_use_local_space_and_exclusion() {
        let w = world(
            ColliderShape::Box {
                half_extents: Vec3::new(0.01, 2., 2.),
            },
            0.7,
        );
        let cast = |world: &dyn WorldView, exclude| {
            cast_sphere(world, Vec3::default(), Vec3::new(10., 0., 0.), 0.2, exclude).unwrap()
        };
        let hit = cast(&w, None);
        assert!((hit - (5. - 0.21 / 0.7_f32.cos()) / 10.).abs() < 1e-5);
        assert_eq!(cast(&w, Some(1)), 1.);
        assert_eq!(cast(&w.snapshot(), None), hit);
        assert_eq!(
            cast_sphere(&w, Vec3::new(5., 0., 0.), Vec3::new(6., 0., 0.), 0.2, None).unwrap(),
            0.
        );
    }
    #[test]
    fn contact_normals_are_outward_world_space_and_overlaps_are_explicit() {
        let w = world(
            ColliderShape::Box {
                half_extents: Vec3::new(0.01, 2., 2.),
            },
            0.7,
        );
        let h = cast_sphere_hit(&w, Vec3::default(), Vec3::new(10., 0., 0.), 0.2, None)
            .unwrap()
            .unwrap();
        let n = h.normal.unwrap();
        assert!((n.dot(n) - 1.).abs() < 1e-5);
        assert!((n.x + 0.7_f32.cos()).abs() < 1e-5 && (n.y + 0.7_f32.sin()).abs() < 1e-5);
        let h = cast_sphere_hit(&w, Vec3::new(5., 0., 0.), Vec3::new(6., 0., 0.), 0.2, None)
            .unwrap()
            .unwrap();
        assert_eq!(h.fraction, 0.);
        assert!(h.normal.is_none());
        assert!(
            cast_sphere_hit(&w, Vec3::default(), Vec3::new(10., 0., 0.), 0.2, Some(1))
                .unwrap()
                .is_none()
        );
        let w = world(ColliderShape::Sphere { radius: 1. }, 0.);
        let n = cast_sphere_hit(&w, Vec3::default(), Vec3::new(10., 0., 0.), 0.2, None)
            .unwrap()
            .unwrap()
            .normal
            .unwrap();
        assert!((n.x + 1.).abs() < 1e-5 && n.y.abs() < 1e-5);
    }
    #[test]
    fn padding_catches_spheres_missed_by_center_line_and_checks_inputs() {
        let w = world(ColliderShape::Sphere { radius: 1. }, 0.);
        let start = Vec3::new(0., 1.1, 0.);
        let end = Vec3::new(10., 1.1, 0.);
        assert!(cast_sphere(&w, start, end, 0.2, None).unwrap() < 0.5);
        assert_eq!(cast_sphere(&w, start, end, 0.02, None).unwrap(), 1.);
        assert!(cast_sphere(&w, start, end, f32::NAN, None).is_err());
        assert!(cast_sphere(&w, start, Vec3::new(f32::MAX, 0., 0.), 0.2, None).is_err());
    }
}
