use crate::*;
use io_types::{Bounds, Vec3, VisualStateId};

#[test]
fn fully_rotated_nonuniform_bounds_contain_every_transformed_corner() {
    let mut item = item();
    item.transform = Transform::oriented(
        Vec3::new(10., 20., 30.),
        Vec3::new(2., 3., 4.),
        io_types::Rotation::from_xyzw([0.3, -0.2, 0.5, 0.7]).unwrap(),
    )
    .unwrap();
    item.occupancy.local_bounds = Bounds {
        min: Vec3::new(-1., -2., -3.),
        max: Vec3::new(2., 1., 1.),
    };
    let m = item.transform();
    let b = item.bounds();
    for x in [-1., 2.] {
        for y in [-2., 1.] {
            for z in [-3., 1.] {
                let p = Vec3::new(
                    m[0] * x + m[4] * y + m[8] * z + m[12],
                    m[1] * x + m[5] * y + m[9] * z + m[13],
                    m[2] * x + m[6] * y + m[10] * z + m[14],
                );
                assert!(p.x >= b.min.x - 1e-4 && p.x <= b.max.x + 1e-4);
                assert!(p.y >= b.min.y - 1e-4 && p.y <= b.max.y + 1e-4);
                assert!(p.z >= b.min.z - 1e-4 && p.z <= b.max.z + 1e-4);
            }
        }
    }
}

fn item() -> Item {
    Item {
        id: 1,
        ..Item::default()
    }
}
fn world(item: Item) -> World {
    World::try_new(Space::new(Vec3::new(100., 100., 100.)), vec![item]).unwrap()
}
fn route() -> PathMotion {
    PathMotion::new(vec![Vec3::default(), Vec3::new(20., 0., 0.)], 2., 0.).unwrap()
}

#[test]
fn baseline_has_no_implicit_visuals_health_or_simulation() {
    let mut world = world(item());
    let before = world.items()[0].clone();
    assert!(before.renderable.is_none() && before.durability.is_none());
    assert!(!before.needs_simulation());
    assert!(!world.damage(1, 10));
    assert!(!world.set_visual_state(1, VisualStateId(0)));
    world.simulate(&[0], 1.);
    assert_eq!(world.items()[0], before);
    assert_eq!(world.revision(), 0);
    assert_eq!(world.query(Vec3::default(), 1.), vec![0]);
}

#[test]
fn movement_does_not_require_a_model_or_health() {
    let mut item = item();
    item.motion = Some(route());
    let mut world = world(item);
    world.simulate(&[0], 1.);
    assert_eq!(world.items()[0].transform.anchor.x, 2.);
    assert_eq!(world.items()[0].render_pose(0.5).0.x, 1.);
    assert_eq!(world.items()[0].simulated_ticks, 1);
    assert!(world.items()[0].renderable.is_none());
}

#[test]
fn depletion_without_a_response_does_not_stop_movement_or_animation() {
    let mut item = item();
    item.motion = Some(route());
    item.renderable = Some(Renderable::default());
    item.animation = Some(AnimationState::looping(0));
    item.durability = Some(Durability::new(1, 10).unwrap());
    let mut world = world(item);
    let spatial_revision = world.spatial_revision();
    assert!(world.damage(1, 100));
    assert_eq!(world.spatial_revision(), spatial_revision);
    world.simulate(&[0], 1.);
    let item = &world.items()[0];
    assert_eq!(item.durability.as_ref().unwrap().current(), 0);
    assert_eq!(item.transform.anchor.x, 2.);
    assert_eq!(item.animation.as_ref().unwrap().time(), 1.);
}

#[test]
fn visual_only_response_keeps_movement_and_is_consumed_once() {
    let mut item = item();
    item.motion = Some(route());
    item.renderable = Some(Renderable {
        visual_state_count: 2,
        ..Renderable::default()
    });
    item.durability = Some(Durability::new(1, 1).unwrap());
    item.depletion_response = Some(DepletionResponse {
        visual_state: Some(VisualStateId(1)),
        ..DepletionResponse::default()
    });
    let mut world = world(item);
    assert!(world.damage(1, 1));
    assert!(world.items()[0].depletion_response.is_none());
    assert_eq!(
        world.items()[0].renderable.as_ref().unwrap().visual_state,
        VisualStateId(1)
    );
    let revision = world.revision();
    assert!(!world.damage(1, 1));
    assert_eq!(world.revision(), revision);
    world.simulate(&[0], 1.);
    assert_eq!(world.items()[0].transform.anchor.x, 2.);
}

#[test]
fn invalid_component_combinations_fail_before_world_insertion() {
    assert!(Durability::new(1, 0).is_err());
    assert!(Durability::new(11, 10).is_err());
    let mut invalid = item();
    invalid.animation = Some(AnimationState::looping(0));
    assert!(invalid.validate().unwrap_err().contains("renderable"));
    invalid.animation = None;
    invalid.depletion_response = Some(DepletionResponse::default());
    assert!(invalid.validate().unwrap_err().contains("durability"));
    invalid.durability = Some(Durability::new(1, 1).unwrap());
    invalid.depletion_response.as_mut().unwrap().visual_state = Some(VisualStateId(1));
    assert!(invalid.validate().is_err());
    invalid.renderable = Some(Renderable::default());
    assert!(invalid.validate().is_err());
    invalid.depletion_response.as_mut().unwrap().visual_state = None;
    for player in [
        AnimationState::looping(0),
        {
            let mut player = AnimationState::once(0, 1.).unwrap();
            player.seek(0.5).unwrap();
            player
        },
        {
            let mut player = AnimationState::once(0, 1.).unwrap();
            player.set_speed(0.).unwrap();
            player
        },
    ] {
        invalid.depletion_response.as_mut().unwrap().animation =
            DepletionAnimation::PlayOnce(player);
        assert!(World::try_new(
            Space::new(Vec3::new(100., 100., 100.)),
            vec![invalid.clone()]
        )
        .is_err());
    }
}

#[test]
fn occupancy_is_indexed_even_when_visuals_are_smaller() {
    let mut item = item();
    item.occupancy.local_bounds = Bounds {
        min: Vec3::new(-50., -1., -1.),
        max: Vec3::new(50., 1., 1.),
    };
    item.renderable = Some(Renderable::default());
    let world = world(item);
    assert_eq!(world.query(Vec3::new(45., 0., 0.), 1.), vec![0]);
}
