use crate::*;
use io_types::{Rotation, Vec3};

fn body(id: u64, z: f32, kind: BodyKind, shape: ColliderShape) -> Item {
    Item {
        id,
        transform: Transform::new(Vec3::new(0., 0., z), Vec3::new(1., 1., 1.), 0.).unwrap(),
        physics_body: Some(PhysicsBody::new(kind)),
        collider: Some(Collider::new(shape)),
        ..Item::default()
    }
}
fn sphere(id: u64, z: f32) -> Item {
    body(
        id,
        z,
        BodyKind::Dynamic,
        ColliderShape::Sphere { radius: 0.5 },
    )
}
fn ground() -> Item {
    body(
        1,
        -0.5,
        BodyKind::Static,
        ColliderShape::Box {
            half_extents: Vec3::new(50., 50., 0.5),
        },
    )
}
fn world(items: Vec<Item>) -> World {
    World::try_new(Space::new(Vec3::new(1000., 1000., 1000.)), items).unwrap()
}
fn advance(world: &mut World, ticks: usize) {
    for _ in 0..ticks {
        world.simulate(&[], 1. / 60.);
        assert!(
            world.physics_error().is_none(),
            "{:?}",
            world.physics_error()
        );
    }
}

#[test]
fn gravity_is_configurable_and_independent_of_visibility_and_mass() {
    let a = sphere(1, 10.);
    let mut b = sphere(2, 10.);
    b.transform.anchor.x = 10.;
    b.physics_body.as_mut().unwrap().mass = 100.;
    let mut w = world(vec![a, b]);
    advance(&mut w, 60);
    assert!((w.items()[0].transform.anchor.z - 5.075).abs() < 0.04);
    assert!((w.items()[0].transform.anchor.z - w.items()[1].transform.anchor.z).abs() < 1e-5);
    let mut a = sphere(1, 10.);
    a.physics_body.as_mut().unwrap().gravity_scale = 0.;
    let mut w = world(vec![a]);
    advance(&mut w, 60);
    assert_eq!(w.items()[0].transform.anchor.z, 10.);
    w.set_physics_settings(PhysicsSettings {
        gravity: Vec3::new(0., 0., -2.),
        ..PhysicsSettings::default()
    })
    .unwrap();
    assert!(w.set_gravity_scale(1, 0.5));
    advance(&mut w, 60);
    assert!((w.items()[0].transform.anchor.z - 9.5).abs() < 0.02);
}
#[test]
fn falling_sphere_settles_on_ground_and_reports_contacts() {
    let mut w = world(vec![ground(), sphere(2, 64.)]);
    let mut contacts = 0;
    for _ in 0..300 {
        advance(&mut w, 1);
        contacts += w.contacts().len();
    }
    assert!(contacts > 0);
    assert!((w.items()[1].transform.anchor.z - 0.5).abs() < 0.025);
    assert!(w.items()[1].physics_body.as_ref().unwrap().velocity.z.abs() < 0.1);
    assert_eq!(w.items()[0].transform.anchor.z, -0.5);
    // World broad-phase buckets are XY; height is rejected by the bounds check.
    assert!(!w.items()[1]
        .visibility_bounds()
        .within_radius(Vec3::new(0., 0., 64.), 0.1));
    assert!(w.query(Vec3::new(0., 0., 0.5), 0.1).contains(&1));
}
#[test]
fn centered_impulse_scales_with_mass_and_off_center_impulse_spins() {
    let mut a = sphere(1, 10.);
    a.physics_body.as_mut().unwrap().mass = 2.;
    let mut w = world(vec![a]);
    assert!(w.apply_impulse(1, Vec3::new(4., 0., 0.), Vec3::new(0., 0., 10.)));
    assert_eq!(w.items()[0].physics_body.as_ref().unwrap().velocity.x, 2.);
    assert_eq!(
        w.items()[0].physics_body.as_ref().unwrap().angular_velocity,
        Vec3::default()
    );
    assert!(w.apply_impulse(1, Vec3::new(0., 2., 0.), Vec3::new(0.5, 0., 10.)));
    assert!(
        w.items()[0]
            .physics_body
            .as_ref()
            .unwrap()
            .angular_velocity
            .z
            > 0.
    );
    let saved = w.items()[0].clone();
    assert!(!w.apply_impulse(1, Vec3::new(f32::NAN, 0., 0.), Vec3::default()));
    assert!(!w.set_pose(1, Vec3::default(), 0.));
    assert_eq!(w.items()[0], saved);
}
#[test]
fn four_box_stack_remains_standing() {
    let mut items = vec![ground()];
    for i in 0..4 {
        items.push(body(
            i + 2,
            0.5 + i as f32 * 1.001,
            BodyKind::Dynamic,
            ColliderShape::Box {
                half_extents: Vec3::new(0.5, 0.5, 0.5),
            },
        ));
    }
    let mut w = world(items);
    advance(&mut w, 600);
    for (i, item) in w.items()[1..].iter().enumerate() {
        assert!(
            (item.transform.anchor.z - (i as f32 + 0.5)).abs() < 0.15,
            "box {i}: {:?}",
            item.transform.anchor
        );
        assert!(item.transform.anchor.x.abs() < 0.2 && item.transform.anchor.y.abs() < 0.2);
    }
}
#[test]
fn collider_filter_can_disable_contact() {
    let mut ball = sphere(2, 1.);
    ball.collider.as_mut().unwrap().filter = 0;
    let mut w = world(vec![ground(), ball]);
    advance(&mut w, 60);
    assert!(w.items()[1].transform.anchor.z < 0.);
    assert!(w.contacts().is_empty());
}
#[test]
fn kinematic_targets_have_single_authority_and_push_dynamic_bodies() {
    let mut kinematic = body(
        1,
        2.,
        BodyKind::Kinematic,
        ColliderShape::Box {
            half_extents: Vec3::new(0.5, 0.5, 0.5),
        },
    );
    kinematic.transform.anchor.x = -1.;
    let mut ball = sphere(2, 2.);
    ball.physics_body.as_mut().unwrap().gravity_scale = 0.;
    let mut w = world(vec![kinematic, ball]);
    for i in 0..60 {
        assert!(w.set_kinematic_target(
            1,
            Vec3::new(-1. + (i + 1) as f32 / 60., 0., 2.),
            Rotation::default()
        ));
        advance(&mut w, 1);
    }
    assert!(w.items()[1].transform.anchor.x > 0.5);
    assert_eq!(w.items()[0].transform.anchor.x, 0.);
    assert!(!w.set_kinematic_target(2, Vec3::default(), Rotation::default()));
}
#[test]
fn invalid_body_combinations_and_materials_are_rejected() {
    let mut item = sphere(1, 1.);
    item.collider = None;
    assert!(World::try_new(Space::new(Vec3::new(10., 10., 10.)), vec![item]).is_err());
    let mut item = sphere(1, 1.);
    item.physics_body.as_mut().unwrap().mass = 0.;
    assert!(item.validate().is_err());
    let mut item = sphere(1, 1.);
    item.collider.as_mut().unwrap().restitution = 2.;
    assert!(item.validate().is_err());
    let mut item = sphere(1, 1.);
    item.motion = PathMotion::new(vec![Vec3::default(), Vec3::new(1., 0., 0.)], 1., 0.);
    assert!(item.validate().is_err());
    assert!(PhysicsSettings {
        substeps: 0,
        ..PhysicsSettings::default()
    }
    .validate()
    .is_err());
}

#[test]
fn physical_motion_updates_spatial_buckets() {
    let mut item = sphere(1, 10.);
    let p = item.physics_body.as_mut().unwrap();
    p.velocity.x = 64.;
    p.gravity_scale = 0.;
    let mut w = world(vec![item]);
    advance(&mut w, 60);
    assert!(!w.query(Vec3::new(0., 0., 10.), 0.1).contains(&0));
    assert!(w.query(Vec3::new(64., 0., 10.), 0.1).contains(&0));
}

#[test]
fn rotation_preserves_offset_center_of_mass_and_visual_scale_does_not_scale_collider() {
    let mut item = sphere(1, 10.);
    item.transform.size = Vec3::new(2., 3., 4.);
    item.collider.as_mut().unwrap().offset = Vec3::new(0., 0., 1.);
    let p = item.physics_body.as_mut().unwrap();
    p.angular_velocity.x = 1.;
    p.angular_damping = 0.;
    p.gravity_scale = 0.;
    let mut w = world(vec![item]);
    advance(&mut w, 60);
    let item = &w.items()[0];
    let center = item.transform.anchor
        + item
            .transform
            .rotation
            .rotate(item.collider.unwrap().offset);
    assert!((center - Vec3::new(0., 0., 11.)).dot(center - Vec3::new(0., 0., 11.)) < 1e-8);
    assert_ne!(item.transform.rotation, Rotation::default());
    let mut item = sphere(2, 4.);
    item.transform.size = Vec3::new(5., 5., 5.);
    let mut w = world(vec![ground(), item]);
    advance(&mut w, 180);
    assert!((w.items()[1].transform.anchor.z - 0.5).abs() < 0.025);
}

#[test]
fn unsupported_result_pauses_physics_without_committing_partial_poses() {
    let mut a = sphere(1, 10.);
    a.physics_body.as_mut().unwrap().velocity.x = 1.;
    let mut b = sphere(2, 10.);
    b.transform.anchor.x = 999999.;
    b.physics_body.as_mut().unwrap().velocity.x = 1000.;
    let mut w = world(vec![a, b]);
    let before: Vec<_> = w.items().iter().map(|i| i.transform).collect();
    w.simulate(&[], 1. / 30.);
    assert!(w.physics_error().is_some());
    assert!(w.contacts().is_empty());
    for (item, transform) in w.items().iter().zip(before) {
        assert_eq!(item.transform, transform);
    }
}

#[test]
fn friction_slows_sliding_boxes_and_restitution_produces_a_bounce() {
    let mut distances = Vec::new();
    for friction in [0., 1.] {
        let mut floor = ground();
        floor.collider.as_mut().unwrap().friction = friction;
        let mut cube = body(
            2,
            0.5,
            BodyKind::Dynamic,
            ColliderShape::Box {
                half_extents: Vec3::new(0.5, 0.5, 0.5),
            },
        );
        cube.collider.as_mut().unwrap().friction = friction;
        cube.physics_body.as_mut().unwrap().velocity.x = 2.;
        let mut w = world(vec![floor, cube]);
        advance(&mut w, 120);
        distances.push(w.items()[1].transform.anchor.x);
    }
    assert!(distances[0] > 3.8 && distances[1] < 1., "{distances:?}");
    let mut ball = sphere(2, 2.);
    ball.collider.as_mut().unwrap().restitution = 0.8;
    let mut w = world(vec![ground(), ball]);
    let mut bounced = false;
    for _ in 0..120 {
        advance(&mut w, 1);
        bounced |= w.items()[1].physics_body.as_ref().unwrap().velocity.z > 2.;
    }
    assert!(bounced);
}

fn stack(id: u64, x: f32) -> Vec<Item> {
    (0..3)
        .map(|i| {
            let mut item = body(
                id + i,
                0.5 + i as f32 * 1.001,
                BodyKind::Dynamic,
                ColliderShape::Box {
                    half_extents: Vec3::new(0.5, 0.5, 0.5),
                },
            );
            item.transform.anchor.x = x;
            item
        })
        .collect()
}

#[test]
fn settled_islands_sleep_and_impulses_wake_only_the_connected_pile() {
    let mut items = vec![ground()];
    items.extend(stack(2, 0.));
    items.extend(stack(5, 10.));
    let mut w = world(items);
    advance(&mut w, 300);
    assert_eq!(w.physics_stats().sleeping, 6);
    assert_eq!(w.physics_stats().islands, 2);
    assert_eq!(w.physics_stats().integrated, 0);
    assert_eq!(w.physics_stats().pair_checks, 0);
    let revision = w.revision();
    let saved = w.items().to_vec();
    advance(&mut w, 10);
    assert_eq!(w.revision(), revision);
    assert_eq!(w.items(), saved);
    assert!(w.apply_impulse(
        2,
        Vec3::new(2., 0., 0.),
        w.item(2).unwrap().transform.anchor
    ));
    advance(&mut w, 1);
    assert_eq!(w.physics_stats().awake, 3);
    assert_eq!(w.physics_stats().sleeping, 3);
    assert_eq!(w.physics_stats().woken, 2);
    assert_eq!(&w.items()[4..], &saved[4..]);
}

#[test]
fn collision_wakes_a_sleeping_chain_before_the_same_substep_solve() {
    let mut items = vec![ground()];
    items.extend(stack(2, 0.));
    let mut ball = sphere(5, 0.5);
    ball.transform.anchor.x = -3.;
    items.push(ball);
    let mut w = world(items);
    advance(&mut w, 300);
    assert_eq!(w.physics_stats().sleeping, 4);
    assert!(w.apply_impulse(
        5,
        Vec3::new(6., 0., 0.),
        w.item(5).unwrap().transform.anchor
    ));
    let mut woke = false;
    for _ in 0..60 {
        advance(&mut w, 1);
        if w.physics_stats().woken >= 3 {
            assert!(w.items()[1..4].iter().all(|i| !i
                .physics_body
                .as_ref()
                .unwrap()
                .is_sleeping()));
            assert!(w.contacts().iter().any(|c| c.a == 5 || c.b == 5));
            woke = true;
            break;
        }
    }
    assert!(woke);
}

#[test]
fn withdrawing_a_kinematic_support_wakes_the_stack_and_gravity_wakes_sleepers() {
    let mut platform = body(
        1,
        -0.5,
        BodyKind::Kinematic,
        ColliderShape::Box {
            half_extents: Vec3::new(1., 1., 0.5),
        },
    );
    platform.physics_body.as_mut().unwrap().gravity_scale = 0.;
    let mut items = vec![platform];
    items.extend(stack(2, 0.));
    let mut w = world(items);
    advance(&mut w, 300);
    assert_eq!(w.physics_stats().sleeping, 3);
    let z = w.item(4).unwrap().transform.anchor.z;
    assert!(w.set_kinematic_target(1, Vec3::new(5., 0., -0.5), Rotation::default()));
    advance(&mut w, 1);
    assert_eq!(w.physics_stats().woken, 3);
    advance(&mut w, 60);
    assert!(w.item(4).unwrap().transform.anchor.z < z - 1.);

    let mut item = sphere(1, 10.);
    item.physics_body.as_mut().unwrap().gravity_scale = 0.;
    let mut w = world(vec![item]);
    advance(&mut w, 120);
    assert_eq!(w.physics_stats().sleeping, 1);
    assert!(w.set_gravity_scale(1, 1.));
    advance(&mut w, 30);
    assert!(w.item(1).unwrap().transform.anchor.z < 9.);
}

#[test]
fn sleep_can_be_disabled_and_unsupported_slow_falling_bodies_do_not_sleep() {
    let mut w = world(vec![ground(), sphere(2, 0.5)]);
    let mut settings = PhysicsSettings::default();
    settings.sleep.enabled = false;
    w.set_physics_settings(settings).unwrap();
    advance(&mut w, 180);
    assert_eq!(w.physics_stats().sleeping, 0);
    assert_eq!(w.physics_stats().integrated, settings.substeps as usize);

    let mut item = sphere(1, 10.);
    item.physics_body.as_mut().unwrap().gravity_scale = 0.0001;
    let mut w = world(vec![item]);
    advance(&mut w, 180);
    assert_eq!(w.physics_stats().sleeping, 0);
    assert!(w.item(1).unwrap().transform.anchor.z < 10.);
    let mut invalid = PhysicsSettings::default();
    invalid.sleep.idle_seconds = f32::NAN;
    assert!(w.set_physics_settings(invalid).is_err());
}

#[test]
fn warm_start_reuses_contacts_and_settings_changes_clear_the_cache() {
    let mut w = world(vec![ground(), sphere(2, 0.5)]);
    let mut settings = PhysicsSettings {
        substeps: 1,
        ..PhysicsSettings::default()
    };
    settings.sleep.enabled = false;
    w.set_physics_settings(settings).unwrap();
    advance(&mut w, 60);
    assert!(w.physics_stats().warm_start_hits > 0);
    assert!(w.physics_stats().cached_contacts > 0);
    assert!(w.set_gravity_scale(2, 0.5));
    advance(&mut w, 1);
    assert_eq!(w.physics_stats().warm_start_hits, 0);
    advance(&mut w, 1);
    assert!(w.physics_stats().warm_start_hits > 0);
    settings.warm_start = false;
    w.set_physics_settings(settings).unwrap();
    advance(&mut w, 10);
    assert_eq!(w.physics_stats().warm_start_hits, 0);
    assert_eq!(w.physics_stats().cached_contacts, 0);
    assert!((w.item(2).unwrap().transform.anchor.z - 0.5).abs() < 0.025);
}

#[test]
fn tgs_six_high_stack_reuses_anchors_and_preserves_support() {
    let mut items = vec![ground()];
    for i in 0..6 {
        items.push(body(
            i + 2,
            0.5 + i as f32 * 1.001,
            BodyKind::Dynamic,
            ColliderShape::Box {
                half_extents: Vec3::new(0.5, 0.5, 0.5),
            },
        ));
    }
    let mut w = world(items);
    let mut settings = PhysicsSettings::tgs();
    settings.sleep.enabled = false;
    w.set_physics_settings(settings).unwrap();
    let mut reuse = 0;
    for _ in 0..300 {
        w.simulate(&[], 1. / 30.);
        assert!(w.physics_error().is_none(), "{:?}", w.physics_error());
        reuse += w.physics_stats().reused_contact_points;
    }
    assert!(reuse > 0);
    for (i, item) in w.items()[1..].iter().enumerate() {
        assert!(
            (item.transform.anchor.z - (i as f32 + 0.5)).abs() < 0.1,
            "{i}: {:?}",
            item.transform.anchor
        );
        assert!(
            item.transform.anchor.x.abs() < 0.1 && item.transform.anchor.y.abs() < 0.1,
            "{i}: {:?}",
            item.transform.anchor
        );
    }
    assert!(w.physics_stats().max_penetration < 0.025);
}

#[test]
fn tgs_detects_incoming_spheres_and_preserves_restitution() {
    let mut ball = sphere(2, 2.);
    ball.physics_body.as_mut().unwrap().velocity.z = -10.;
    ball.collider.as_mut().unwrap().restitution = 0.8;
    let mut w = world(vec![ground(), ball]);
    w.set_physics_settings(PhysicsSettings::tgs()).unwrap();
    let mut bounced = false;
    for _ in 0..60 {
        advance(&mut w, 1);
        let item = w.item(2).unwrap();
        assert!(item.transform.anchor.z > 0.4);
        bounced |= item.physics_body.as_ref().unwrap().velocity.z > 3.;
    }
    assert!(bounced);
}

#[test]
fn tgs_cold_resting_floor_has_no_repeated_broad_phase_builds() {
    let mut settings = PhysicsSettings::tgs();
    settings.warm_start = false;
    settings.sleep.enabled = false;
    let mut w = world(vec![ground(), sphere(2, 0.5)]);
    w.set_physics_settings(settings).unwrap();
    advance(&mut w, 180);
    assert_eq!(w.physics_stats().broad_phase_builds, 1);
    assert!(w.physics_stats().reused_contact_points > 0);
    assert!((w.item(2).unwrap().transform.anchor.z - 0.5).abs() < 0.025);
    assert_eq!(w.physics_stats().cached_contacts, 0);
}

#[test]
fn diagnostic_toggle_preserves_world_and_tick_counts() {
    let items = vec![ground(), sphere(2, 0.5), sphere(3, 1.51)];
    let mut reference = world(items.clone());
    let mut observed = world(items);
    assert!(observed.contact_diagnostics().is_none());
    observed.set_contact_diagnostics(true);
    for _ in 0..180 {
        reference.simulate(&[], 1. / 30.);
        observed.simulate(&[], 1. / 30.);
        assert!(observed.physics_error().is_none());
        for (a, b) in reference.items().iter().zip(observed.items()) {
            assert_eq!(a.transform.anchor, b.transform.anchor);
            assert_eq!(a.transform.rotation, b.transform.rotation);
            assert_eq!(a.physics_body, b.physics_body);
        }
        assert_eq!(reference.contacts().len(), observed.contacts().len());
        for (a, b) in reference.contacts().iter().zip(observed.contacts()) {
            assert_eq!(
                (a.a, a.b, a.point, a.normal, a.impulse),
                (b.a, b.b, b.point, b.normal, b.impulse)
            );
        }
        for pass in &observed.contact_diagnostics().unwrap().passes {
            assert_eq!(pass.visits, observed.physics_stats().contacts as u64);
            assert_eq!(pass.correction_histogram.iter().sum::<u64>(), pass.visits);
            assert_eq!(pass.end_pass_histogram.iter().sum::<u64>(), pass.visits);
            assert_eq!(pass.invalid_samples, 0);
        }
    }
    observed.set_contact_diagnostics(false);
    assert!(observed.contact_diagnostics().is_none());
}

#[test]
fn warm_start_keeps_a_six_high_stack_stable_at_four_iterations_without_sleep() {
    let mut items = vec![ground()];
    for i in 0..6 {
        items.push(body(
            i + 2,
            0.5 + i as f32 * 1.001,
            BodyKind::Dynamic,
            ColliderShape::Box {
                half_extents: Vec3::new(0.5, 0.5, 0.5),
            },
        ));
    }
    let mut w = world(items);
    let mut settings = PhysicsSettings {
        iterations: 4,
        ..PhysicsSettings::default()
    };
    settings.sleep.enabled = false;
    w.set_physics_settings(settings).unwrap();
    let mut hits = 0;
    for _ in 0..300 {
        w.simulate(&[], 1. / 30.);
        assert!(w.physics_error().is_none());
        hits += w.physics_stats().warm_start_hits;
    }
    assert!(hits > 0);
    assert_eq!(w.physics_stats().sleeping, 0);
    for (i, item) in w.items()[1..].iter().enumerate() {
        assert!((item.transform.anchor.z - (i as f32 + 0.5)).abs() < 0.1);
        assert!(
            item.transform.anchor.x.abs() < 0.1 && item.transform.anchor.y.abs() < 0.1,
            "stack item {i}: {:?}",
            item.transform.anchor
        );
    }
    assert!(w.physics_stats().max_penetration < 0.025);
}
