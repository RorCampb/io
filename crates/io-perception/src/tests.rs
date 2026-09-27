use super::*;
use crate::pipeline::*;
use io_game::stage::Stage;
use io_world::{
    BodyKind, CharacterBody, Collider, ColliderShape, Item, PhysicsBody, Space, Transform, World,
};

#[derive(Clone, Default)]
struct Track {
    cache: ObservationCache,
    awareness: Awareness,
}
impl Track {
    fn update(&mut self, w: &dyn WorldView, threshold: f32) -> ObservationResult {
        RelevanceStage
            .then(EvidenceStage)
            .then(AttentionStage)
            .run(ObservationRequest {
                world: w,
                observer: 1,
                target: 2,
                vision: profile(),
                seconds: 0.1,
                minimum_attention: threshold,
                cache: &mut self.cache,
                awareness: &mut self.awareness,
            })
            .unwrap()
    }
}

#[test]
fn stages_reuse_evidence_but_keep_integrating_attention_without_new_changes() {
    let w = world();
    let mut track = Track::default();
    let first = track.update(&w, 0.4);
    assert!(first.resampled && first.notice.is_none());
    let mut noticed = false;
    for _ in 0..30 {
        let result = track.update(&w, 0.4);
        assert!(!result.resampled);
        noticed |= result
            .notice
            .is_some_and(|n| n.kind == NoticeKind::Acquired);
    }
    assert!(noticed);
    assert!(track.update(&w, 0.4).notice.is_none());
}

#[test]
fn unrelated_items_do_not_resample_and_hidden_target_changes_do_not_leak() {
    let mut items = world().items().to_vec();
    items.push(Item {
        id: 3,
        ..Default::default()
    });
    let mut w = World::new(Space::new(Vec3::new(100., 100., 20.)), items);
    let mut track = Track::default();
    let original = track.update(&w, 0.).notice.unwrap().bounds;
    w.set_pose(3, Vec3::new(70., 70., 0.), 0.);
    let unchanged = track.update(&w, 0.);
    assert!(!unchanged.resampled && unchanged.notice.is_none());
    w.set_pose(2, Vec3::new(0., 12., 0.), 0.);
    let hidden = track.update(&w, 0.);
    assert!(hidden.resampled && hidden.contact.is_none() && hidden.notice.is_none());
    let mut old_track = track.clone();
    let old_world = w.snapshot();
    w.set_pose(2, Vec3::new(0., 1., 0.), 0.);
    let visible = track.update(&w, 0.);
    assert_eq!(visible.notice.unwrap().kind, NoticeKind::Changed);
    assert_eq!(visible.notice.unwrap().previous_bounds, Some(original));
    assert_eq!(
        visible.notice.unwrap().bounds,
        w.item(2).unwrap().current_spatial_bounds()
    );
    assert!(old_track.update(&old_world, 0.).contact.is_none());
}

#[test]
fn physical_items_are_observable_without_character_bodies_and_do_not_occlude_themselves() {
    let mut items = world().items().to_vec();
    items[1].character_body = None;
    items[1].collider = Some(Collider::new(ColliderShape::Box {
        half_extents: Vec3::new(1., 1., 1.),
    }));
    items[1].physics_body = Some(PhysicsBody::new(BodyKind::Kinematic));
    let mut w = World::new(Space::new(Vec3::new(100., 100., 20.)), items);
    let mut track = Track::default();
    assert!(track.update(&w, 0.).notice.is_some());
    let snapshot = w.snapshot();
    let cursor = w.changes().cursor();
    w.set_kinematic_target(2, Vec3::new(1., 0., 0.), io_types::Rotation::default());
    // A requested physics target is not yet an observed displacement.
    assert_eq!(w.changes().cursor(), cursor);
    w.simulate(&[], 1. / 60.);
    assert!(w
        .changes()
        .since(cursor)
        .unwrap()
        .any(|c| matches!(c.source, io_world::ChangeSource::Item(2))));
    assert_eq!(snapshot.changes().unwrap().cursor(), cursor);
    assert_eq!(
        track.update(&w, 0.).notice.unwrap().kind,
        NoticeKind::Changed
    );
}

#[test]
fn moving_occluder_resamples_existing_pair_using_old_and_new_bounds() {
    let mut items = world().items().to_vec();
    items.push(Item {
        id: 3,
        transform: Transform::new(Vec3::new(0., 4., 1.3), Vec3::new(2., 0.6, 2.6), 0.).unwrap(),
        collider: Some(Collider::new(ColliderShape::Box {
            half_extents: Vec3::new(1., 0.3, 1.3),
        })),
        physics_body: Some(PhysicsBody::new(BodyKind::Kinematic)),
        ..Default::default()
    });
    let mut w = World::new(Space::new(Vec3::new(100., 100., 20.)), items);
    let mut track = Track::default();
    assert!(track.update(&w, 0.).contact.is_none());
    w.set_kinematic_target(3, Vec3::new(10., 4., 1.3), io_types::Rotation::default());
    w.simulate(&[], 1. / 60.);
    let opened = track.update(&w, 0.);
    assert!(opened.resampled && opened.contact.is_some());
    // Let interpolation history settle; broad-phase records conservatively include it.
    w.simulate(&[], 1. / 60.);
    w.set_kinematic_target(3, Vec3::new(12., 4., 1.3), io_types::Rotation::default());
    w.simulate(&[], 1. / 60.);
    assert!(!track.update(&w, 0.).resampled);
    w.set_kinematic_target(3, Vec3::new(0., 4., 1.3), io_types::Rotation::default());
    w.simulate(&[], 1. / 60.);
    let closed = track.update(&w, 0.);
    assert!(closed.resampled && closed.contact.is_none() && closed.notice.is_none());
}

#[test]
fn missing_change_history_resynchronizes_and_invalid_requests_preserve_attention() {
    let mut w = world();
    let mut track = Track::default();
    track.update(&w, 0.);
    for i in 0..4100 {
        w.set_pose(2, Vec3::new((i % 2) as f32, 0., 0.), 0.);
    }
    let result = track.update(&w, 0.);
    assert!(result.resampled);
    assert_eq!(result.notice.unwrap().kind, NoticeKind::Resynchronized);
    let before = track.awareness.value();
    let result = RelevanceStage
        .then(EvidenceStage)
        .then(AttentionStage)
        .run(ObservationRequest {
            world: &w,
            observer: 1,
            target: 2,
            vision: profile(),
            seconds: f32::NAN,
            minimum_attention: 0.2,
            cache: &mut track.cache,
            awareness: &mut track.awareness,
        });
    assert!(result.is_err());
    assert_eq!(track.awareness.value(), before);
    assert!(!track.update(&w, 0.).resampled);
}

#[test]
fn observation_cache_cannot_alias_another_world_with_identical_item_ids() {
    let w = world();
    let other = world();
    let mut track = Track::default();
    track.update(&w, 0.);
    let result = RelevanceStage.run(ObservationRequest {
        world: &other,
        observer: 1,
        target: 2,
        vision: profile(),
        seconds: 0.1,
        minimum_attention: 0.,
        cache: &mut track.cache,
        awareness: &mut track.awareness,
    });
    assert!(result.is_err());
}

#[test]
fn non_character_samples_use_current_pose_not_previous_interpolation_bounds() {
    let mut items = world().items().to_vec();
    items[1].character_body = None;
    items[1].collider = Some(Collider::new(ColliderShape::Box {
        half_extents: Vec3::new(0.5, 0.5, 0.5),
    }));
    items[1].physics_body = Some(PhysicsBody::new(BodyKind::Kinematic));
    let mut w = World::new(Space::new(Vec3::new(100., 100., 20.)), items);
    let mut track = Track::default();
    assert!(track.update(&w, 0.).contact.is_some());
    w.set_kinematic_target(2, Vec3::new(0., 20., 0.), io_types::Rotation::default());
    w.simulate(&[], 1. / 60.);
    let hidden = track.update(&w, 0.);
    assert!(hidden.contact.is_none() && hidden.notice.is_none());
}

fn profile() -> VisionProfile {
    VisionProfile {
        range: 24.,
        fov_degrees: 120.,
        gain_per_second: 2.4,
        decay_per_second: 0.35,
    }
}

#[test]
fn wide_fov_preserves_smooth_peripheral_falloff_including_beside_observer() {
    let mut w = world();
    w.set_pose(1, Vec3::default(), 0.);
    let wide = VisionProfile {
        fov_degrees: 240.,
        ..profile()
    };
    let mut previous = 1.;
    for degrees in 0..=130 {
        let angle = (degrees as f32).to_radians();
        w.set_pose(2, Vec3::new(angle.sin() * 5., -angle.cos() * 5., 0.), 0.);
        let evidence = observe(&w, 1, 2, wide).unwrap().evidence;
        assert!(
            evidence <= previous + 0.001,
            "angle={degrees}, evidence={evidence}"
        );
        if degrees == 90 {
            assert!(evidence > 0.15);
            assert_eq!(observe(&w, 1, 2, profile()).unwrap().evidence, 0.);
        }
        if degrees == 130 {
            assert_eq!(evidence, 0.);
        }
        previous = evidence;
    }
    for fov in [5., 180., 240., 360.] {
        assert!(VisionProfile {
            fov_degrees: fov,
            ..profile()
        }
        .validate()
        .is_ok());
    }
    for fov in [f32::NAN, 0., 361.] {
        assert!(VisionProfile {
            fov_degrees: fov,
            ..profile()
        }
        .validate()
        .is_err());
    }
}
fn world() -> World {
    let actor = |id, p| Item {
        id,
        transform: Transform::new(p, Vec3::new(1., 1., 1.), 0.).unwrap(),
        character_body: Some(CharacterBody {
            radius: 0.35,
            height: 1.9,
            max_slope: 0.8,
        }),
        ..Default::default()
    };
    World::new(
        Space::new(Vec3::new(100., 100., 20.)),
        vec![actor(1, Vec3::new(0., 8., 0.)), actor(2, Vec3::default())],
    )
}
#[test]
fn constant_evidence_is_independent_of_tick_rate_and_invalid_input_preserves_state() {
    let integrate = |hz| {
        let mut memory = Awareness::default();
        for _ in 0..hz * 5 {
            memory.advance(0.6, profile(), 1. / hz as f32).unwrap();
        }
        memory
    };
    let mut slow = integrate(30);
    assert!((slow.value() - integrate(144).value()).abs() < 1e-6);
    let before = slow.value();
    assert!(slow.advance(f32::NAN, profile(), 0.1).is_err());
    assert_eq!(slow.value(), before);
    let zero = VisionProfile {
        gain_per_second: 0.,
        decay_per_second: 0.,
        ..profile()
    };
    slow.advance(1., zero, 1.).unwrap();
    assert_eq!(slow.value(), before);
}
#[test]
fn brief_glimpse_is_small_sustained_visibility_grows_and_cover_decays() {
    let mut memory = Awareness::default();
    memory.advance(0.1, profile(), 0.05).unwrap();
    assert!(memory.value() < 0.02);
    for _ in 0..180 {
        memory.advance(0.8, profile(), 1. / 60.).unwrap();
    }
    let high = memory.value();
    assert!(high > 0.79 && high <= 0.8);
    for _ in 0..180 {
        memory.advance(0., profile(), 1. / 60.).unwrap();
    }
    assert!(memory.value() < high * 0.4 && memory.value() > 0.);
}
#[test]
fn fov_and_distance_reduce_evidence_and_blocked_rays_reduce_exposure() {
    let mut world = world();
    let centered = observe(&world, 1, 2, profile()).unwrap();
    assert!(centered.exposure > 0.99 && centered.evidence > 0.5);
    world.set_pose(2, Vec3::new(6., 0., 0.), 0.);
    assert!(observe(&world, 1, 2, profile()).unwrap().evidence < centered.evidence);
    world.set_pose(2, Vec3::new(0., 10., 0.), 0.);
    assert_eq!(observe(&world, 1, 2, profile()).unwrap().exposure, 0.);
    world.set_pose(2, Vec3::new(0., -30., 0.), 0.);
    assert_eq!(observe(&world, 1, 2, profile()).unwrap().evidence, 0.);
    assert!(observe(&world, 1, 1, profile()).is_err());
    assert!(observe(&world, 1, 99, profile()).is_err());
}
#[test]
fn moving_past_a_barrier_produces_hidden_partial_and_full_exposure() {
    let mut items = world().items().to_vec();
    items.push(Item {
        id: 3,
        transform: Transform::new(Vec3::new(0., 4., 1.3), Vec3::new(2., 0.6, 2.6), 0.).unwrap(),
        collider: Some(Collider::new(ColliderShape::Box {
            half_extents: Vec3::new(1., 0.3, 1.3),
        })),
        physics_body: Some(PhysicsBody::new(BodyKind::Static)),
        ..Default::default()
    });
    let mut world = World::new(Space::new(Vec3::new(100., 100., 20.)), items);
    assert_eq!(observe(&world, 1, 2, profile()).unwrap().exposure, 0.);
    let mut partial = false;
    let mut full = false;
    for step in 0..100 {
        world.set_pose(2, Vec3::new(step as f32 * 0.05, 0., 0.), 0.);
        let exposure = observe(&world, 1, 2, profile()).unwrap().exposure;
        partial |= exposure > 0. && exposure < 0.99;
        full |= exposure > 0.99;
    }
    assert!(partial && full);
}
#[test]
fn observers_hold_independent_memory_and_bad_profiles_are_rejected() {
    let mut a = Awareness::default();
    let b = Awareness::default();
    a.advance(1., profile(), 1.).unwrap();
    assert!(a.value() > 0.5 && b.value() == 0.);
    for bad in [
        VisionProfile {
            fov_degrees: 0.,
            ..profile()
        },
        VisionProfile {
            gain_per_second: -1.,
            ..profile()
        },
        VisionProfile {
            range: f32::NAN,
            ..profile()
        },
    ] {
        assert!(bad.validate().is_err());
    }
}

#[test]
fn rear_targets_never_supply_evidence_as_observer_rotates() {
    let mut world = world();
    let origin = Vec3::new(0., 8., 0.);
    for yaw in [0., 0.7, 1.8, 3.5, 5.2] {
        world.set_pose(1, origin, yaw);
        let rotation = io_types::Rotation::yaw(yaw).unwrap();
        for degrees in [100_f32, 135., 180., 225., 260.] {
            let angle = degrees.to_radians();
            let offset = rotation.rotate(Vec3::new(angle.sin(), -angle.cos(), 0.));
            for distance in [0.8, 3., 10.] {
                world.set_pose(2, origin + offset.scaled(distance), yaw + 1.);
                let observation = observe(&world, 1, 2, profile()).unwrap();
                assert_eq!(observation.exposure, 0., "yaw={yaw}, angle={degrees}");
                assert_eq!(observation.evidence, 0.);
                let mut memory = Awareness::default();
                memory.advance(1., profile(), 1.).unwrap();
                let before = memory.value();
                memory
                    .advance(observation.evidence, profile(), 0.1)
                    .unwrap();
                assert!(memory.value() < before);
            }
        }
        world.set_pose(2, origin + rotation.rotate(Vec3::new(0., -3., 0.)), yaw);
        assert!(observe(&world, 1, 2, profile()).unwrap().evidence > 0.8);
        assert_eq!(observe(&world, 2, 1, profile()).unwrap().evidence, 0.);
    }
}

#[test]
fn nearby_front_target_does_not_lose_lower_body_samples_to_vertical_angle() {
    let mut world = world();
    let far = observe(&world, 1, 2, profile()).unwrap();
    for distance in [0.75, 0.8, 1., 2., 3.] {
        world.set_pose(2, Vec3::new(0., 8. - distance, 0.), 0.);
        let near = observe(&world, 1, 2, profile()).unwrap();
        assert!(near.exposure > 0.99, "distance={distance}: {near:?}");
        assert!(near.evidence > 0.8, "distance={distance}: {near:?}");
        assert!(near.evidence > far.evidence);
    }
}

#[test]
fn focus_tapers_continuously_from_model_front_to_peripheral_edge() {
    let edge = (profile().fov_degrees.to_radians() * 0.5).cos();
    assert_eq!(directional_focus(1., edge), 1.);
    let mut previous = 1.;
    for degrees in 1..=180 {
        let score = directional_focus((degrees as f32).to_radians().cos(), edge);
        assert!(score <= previous, "angle={degrees}");
        if degrees >= 60 {
            assert_eq!(score, 0.);
        }
        previous = score;
    }
    assert!(directional_focus(59_f32.to_radians().cos(), edge) < 0.003);

    let mut world = world();
    let origin = Vec3::new(0., 8., 0.);
    for yaw in [0., 0.7, 2.4] {
        world.set_pose(1, origin, yaw);
        let rotation = io_types::Rotation::yaw(yaw).unwrap();
        for side in [-1., 1.] {
            let mut previous = 1.;
            for degrees in [0_f32, 15., 30., 45., 55., 65., 90., 180.] {
                let angle = side * degrees.to_radians();
                let delta = rotation.rotate(Vec3::new(angle.sin(), -angle.cos(), 0.));
                // Rotate the target with its position to keep sample geometry comparable.
                world.set_pose(2, origin + delta.scaled(5.), yaw + angle);
                let evidence = observe(&world, 1, 2, profile()).unwrap().evidence;
                assert!(evidence <= previous, "yaw={yaw}, angle={degrees}");
                previous = evidence;
            }
        }
    }
}

#[test]
fn sustained_peripheral_focus_stays_weak_and_lost_focus_reduces_attention() {
    for hz in [30, 60, 144] {
        let mut peripheral = Awareness::default();
        for _ in 0..hz * 30 {
            peripheral.advance(0.1, profile(), 1. / hz as f32).unwrap();
            assert!(peripheral.value() <= 0.1);
        }
        assert!((peripheral.value() - 0.1).abs() < 1e-6);

        let mut focused = Awareness::default();
        focused.advance(1., profile(), 1.).unwrap();
        let mut previous = focused.value();
        for _ in 0..hz * 30 {
            focused.advance(0.1, profile(), 1. / hz as f32).unwrap();
            assert!(focused.value() <= previous && focused.value() >= 0.1);
            previous = focused.value();
        }
        assert!((focused.value() - 0.1).abs() < 0.00003);
        for _ in 0..hz * 30 {
            focused.advance(0., profile(), 1. / hz as f32).unwrap();
        }
        assert!(focused.value() < 0.00001);
    }
}
