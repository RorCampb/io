//! Camera eye movement against actual collider contact planes, not world axes.
use io_types::Vec3;
use io_world::WorldView;

pub fn slide(
    world: &dyn WorldView,
    start: Vec3,
    delta: Vec3,
    radius: f32,
    exclude: Option<u64>,
) -> Option<Vec3> {
    let mut position = start;
    let mut remaining = delta;
    let mut planes = [Vec3::default(); 4];
    let mut count = 0;
    for _ in 0..4 {
        let length = remaining.dot(remaining).sqrt();
        if length < 1e-6 {
            break;
        }
        let hit =
            match io_world::cast_sphere_hit(world, position, position + remaining, radius, exclude)
            {
                Ok(None) => return (count > 0).then_some(position + remaining),
                Ok(Some(hit)) => hit,
                Err(_) => break,
            };
        let travel = (hit.fraction - 0.006 / length).max(0.);
        position = position + remaining.scaled(travel);
        let Some(normal) = hit.normal else {
            break;
        };
        // Keep ceiling/floor behavior unchanged while adding wall sliding.
        if normal.z.abs() > 0.7 {
            break;
        }
        planes[count] = normal;
        count += 1;
        let mut tangent = remaining;
        for n in &planes[..count] {
            tangent = tangent - n.scaled(tangent.dot(*n).min(0.));
        }
        let tangent_length = tangent.dot(tangent).sqrt();
        if tangent_length < length * 1e-4 || planes[..count].iter().any(|n| tangent.dot(*n) < -1e-5)
        {
            break;
        }
        // Preserve the unspent path length rather than slowing by cos(incidence).
        remaining = tangent.scaled(length * (1. - travel) / tangent_length);
    }
    (count > 0).then_some(position)
}

#[cfg(test)]
mod tests {
    use super::*;
    use io_world::{BodyKind, Collider, ColliderShape, Item, PhysicsBody, Space, Transform, World};
    fn wall(id: u64, anchor: Vec3, half: Vec3, yaw: f32) -> Item {
        Item {
            id,
            transform: Transform::new(anchor, Vec3::new(1., 1., 1.), yaw).unwrap(),
            collider: Some(Collider::new(ColliderShape::Box { half_extents: half })),
            physics_body: Some(PhysicsBody::new(BodyKind::Static)),
            ..Default::default()
        }
    }
    #[test]
    fn wall_slide_preserves_speed_in_world_space_including_rotated_walls() {
        for yaw in [0., 0.7, 1.5] {
            let rotation = io_types::Rotation::yaw(yaw).unwrap();
            let w = World::new(
                Space::new(Vec3::new(100., 100., 100.)),
                vec![wall(1, Vec3::default(), Vec3::new(0.1, 50., 50.), yaw)],
            );
            let start = rotation.rotate(Vec3::new(-0.226, 0., 0.));
            let delta = rotation.rotate(Vec3::new(0.1, 0.1, 0.));
            let end = slide(&w, start, delta, 0.12, None).unwrap();
            let local = rotation.inverse_rotate(end);
            assert!(local.x <= -0.22);
            assert!(
                (local.y - delta.dot(delta).sqrt()).abs() < 0.002,
                "wall slowed tangential travel: {local:?}"
            );
            assert_eq!(
                io_world::cast_sphere(&w, start, end, 0.12, None).unwrap(),
                1.
            );
            assert!(
                slide(
                    &w,
                    end,
                    rotation.rotate(Vec3::new(-0.1, 0., 0.)),
                    0.12,
                    None
                )
                .is_none(),
                "moving away releases immediately"
            );
        }
    }
    #[test]
    fn head_on_and_inside_corner_stop_without_choosing_an_arbitrary_direction() {
        let w = World::new(
            Space::new(Vec3::new(100., 100., 100.)),
            vec![
                wall(1, Vec3::default(), Vec3::new(0.1, 50., 50.), 0.),
                wall(2, Vec3::default(), Vec3::new(50., 0.1, 50.), 0.),
            ],
        );
        let start = Vec3::new(-0.226, -0.226, 0.);
        for delta in [Vec3::new(0.1, 0., 0.), Vec3::new(0.1, 0.1, 0.)] {
            let end = slide(&w, start, delta, 0.12, None).unwrap();
            assert!(end.x <= -0.22 && end.y <= -0.22);
            assert!((end - start).dot(end - start) < 0.0001);
        }
    }
}
