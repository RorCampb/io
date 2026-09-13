use crate::*;
use io_types::Vec3;

fn damageable() -> Item {
    Item {
        renderable: Some(Renderable::default()),
        durability: Some(Durability::new(100, 100).unwrap()),
        ..Item::default()
    }
}

fn event(at: f64, amount: u32) -> AnimationEvent {
    AnimationEvent {
        name: "impact".into(),
        at,
        target: 42,
        effect: EffectKind::Damage(Damage { amount }),
    }
}

fn world(at: f64) -> World {
    let mut animation = AnimationState::looping(0);
    animation
        .set_events(AnimationEvents::new(1., vec![event(at, 10)]).unwrap())
        .unwrap();
    World::new(
        Space::new(Vec3::new(100., 100., 100.)),
        vec![
            Item {
                id: 7,
                animation: Some(animation),
                ..damageable()
            },
            Item {
                id: 42,
                ..damageable()
            },
        ],
    )
}

#[test]
fn event_fires_once_at_boundary_and_can_damage_an_inactive_target() {
    let mut world = world(0.5);
    world.simulate(&[0], 0.25);
    assert_eq!(
        world
            .item(42)
            .unwrap()
            .durability
            .as_ref()
            .unwrap()
            .current(),
        100
    );
    world.simulate(&[0, 0], 0.25);
    assert_eq!(
        world
            .item(42)
            .unwrap()
            .durability
            .as_ref()
            .unwrap()
            .current(),
        90
    );
    world.simulate(&[0], 0.25);
    assert_eq!(
        world
            .item(42)
            .unwrap()
            .durability
            .as_ref()
            .unwrap()
            .current(),
        90
    );
    assert_eq!(world.item(42).unwrap().simulated_ticks, 0);
}

#[test]
fn loop_end_events_and_multiple_loops_are_not_skipped() {
    let mut world = world(1.);
    world.simulate(&[0], 1.);
    assert_eq!(
        world
            .item(42)
            .unwrap()
            .durability
            .as_ref()
            .unwrap()
            .current(),
        90
    );
    world.simulate(&[0], 2.5);
    assert_eq!(
        world
            .item(42)
            .unwrap()
            .durability
            .as_ref()
            .unwrap()
            .current(),
        70
    );
    world.simulate(&[0], 0.5);
    assert_eq!(
        world
            .item(42)
            .unwrap()
            .durability
            .as_ref()
            .unwrap()
            .current(),
        60
    );
}

#[test]
fn inactive_source_pauses_events_and_resumes_without_catchup() {
    let mut world = world(0.5);
    world.simulate(&[0], 0.25);
    world.simulate(&[], 10.);
    assert_eq!(
        world
            .item(42)
            .unwrap()
            .durability
            .as_ref()
            .unwrap()
            .current(),
        100
    );
    world.simulate(&[0], 0.25);
    assert_eq!(
        world
            .item(42)
            .unwrap()
            .durability
            .as_ref()
            .unwrap()
            .current(),
        90
    );
}

#[test]
fn playback_speed_controls_events_and_instances_are_independent() {
    let source = world(0.5).items()[0].clone();
    let mut second = source.clone();
    second.id = 8;
    second.animation.as_mut().unwrap().set_speed(2.).unwrap();
    let mut world = World::new(
        Space::new(Vec3::new(100., 100., 100.)),
        vec![
            source,
            second,
            Item {
                id: 42,
                ..damageable()
            },
        ],
    );
    world.simulate(&[1, 0], 0.25);
    assert_eq!(
        world
            .item(42)
            .unwrap()
            .durability
            .as_ref()
            .unwrap()
            .current(),
        90
    );
    world.simulate(&[1, 0], 0.25);
    assert_eq!(
        world
            .item(42)
            .unwrap()
            .durability
            .as_ref()
            .unwrap()
            .current(),
        80
    );
}

#[test]
fn damage_saturates_and_invalid_targets_do_not_mutate_world() {
    let mut world = world(0.5);
    let revision = world.revision();
    assert!(!Damage { amount: 10 }.apply(999, &mut world));
    assert!(!Damage { amount: 0 }.apply(42, &mut world));
    assert_eq!(world.revision(), revision);
    assert!(Damage { amount: u32::MAX }.apply(42, &mut world));
    assert_eq!(
        world
            .item(42)
            .unwrap()
            .durability
            .as_ref()
            .unwrap()
            .current(),
        0
    );
    assert!(world.revision() > revision);
    let revision = world.revision();
    assert!(!Damage { amount: 1 }.apply(42, &mut world));
    assert_eq!(world.revision(), revision);
}

#[test]
fn invalid_tracks_are_rejected_and_invalid_playback_cannot_repeat_damage() {
    for duration in [0., -1., f64::NAN, f64::INFINITY] {
        assert!(AnimationEvents::new(duration, vec![event(0.5, 10)]).is_err());
    }
    for time in [0., -1., 1.1, f64::NAN, f64::INFINITY] {
        assert!(AnimationEvents::new(1., vec![event(time, 10)]).is_err());
    }
    assert!(AnimationEvents::new(1., vec![event(0.5, 10), event(0.75, 10)]).is_err());
    for speed in [0., -1., f32::NAN, f32::INFINITY] {
        let mut source = world(0.5).items()[0].clone();
        let animation = source.animation.as_mut().unwrap();
        animation.set_speed(0.).unwrap();
        assert_eq!(animation.set_speed(speed).is_ok(), speed == 0.);
        let mut world = World::new(
            Space::new(Vec3::new(100., 100., 100.)),
            vec![
                source,
                Item {
                    id: 42,
                    ..damageable()
                },
            ],
        );
        world.simulate(&[0], 1.);
        assert_eq!(
            world
                .item(42)
                .unwrap()
                .durability
                .as_ref()
                .unwrap()
                .current(),
            100
        );
    }
}

#[test]
fn queue_contains_each_crossed_event_including_loop_wrap() {
    let mut later = event(0.75, 20);
    later.name = "later".into();
    let track = AnimationEvents::new(1., vec![later, event(0.25, 10)]).unwrap();
    let mut queue = Vec::new();
    track.enqueue(7, 0.5, 1.5, 2., &mut queue);
    queue.sort_by(|a, b| a.offset.total_cmp(&b.offset));
    assert_eq!(
        queue.iter().map(|p| p.offset).collect::<Vec<_>>(),
        vec![0.125, 0.375]
    );
    assert_eq!(
        queue.iter().map(|p| p.event_index).collect::<Vec<_>>(),
        vec![1, 0]
    );
}

#[test]
fn death_stops_movement_and_transitions_once_even_for_an_inactive_item() {
    let mut source = world(0.5).items()[0].clone();
    source.durability = Some(Durability::new(10, 100).unwrap());
    source.depletion_response = Some(DepletionResponse {
        stop_motion: true,
        animation: DepletionAnimation::PlayOnce(AnimationState::once(1, 1.5).unwrap()),
        ..DepletionResponse::default()
    });
    source.motion = PathMotion::new(vec![Vec3::default(), Vec3::new(100., 0., 0.)], 10., 0.);
    let mut world = World::new(Space::new(Vec3::new(200., 200., 20.)), vec![source]);
    world.simulate(&[0], 0.25);
    let anchor = world.item(7).unwrap().transform.anchor;
    assert!(Damage { amount: 10 }.apply(7, &mut world));
    let dead = world.item(7).unwrap();
    assert!(dead.motion.is_none() && dead.depletion_response.is_none());
    assert_eq!(dead.animation.as_ref().unwrap().clip(), 1);
    assert!(dead.animation.as_ref().unwrap().events().is_none());
    assert_eq!(world.query(anchor, 1.), vec![0]);
    world.simulate(&[], 10.);
    assert_eq!(
        world.item(7).unwrap().animation.as_ref().unwrap().time(),
        0.
    );
    world.simulate(&[0], 2.);
    assert!(!Damage { amount: 10 }.apply(7, &mut world));
    assert_eq!(
        world.item(7).unwrap().animation.as_ref().unwrap().time(),
        1.5
    );
    assert_eq!(world.item(7).unwrap().transform.anchor, anchor);
    world.simulate(&[0], 10.);
    assert_eq!(
        world
            .item(7)
            .unwrap()
            .animation
            .as_ref()
            .unwrap()
            .sample_time(0.5, 1.5),
        1.5
    );
}

#[test]
fn initially_dead_items_start_their_death_response() {
    let mut source = world(0.5).items()[0].clone();
    source.durability = Some(Durability::new(0, 100).unwrap());
    source.depletion_response = Some(DepletionResponse {
        stop_motion: true,
        animation: DepletionAnimation::PlayOnce(AnimationState::once(1, 1.5).unwrap()),
        ..DepletionResponse::default()
    });
    let world = World::new(Space::new(Vec3::new(10., 10., 10.)), vec![source]);
    assert_eq!(world.item(7).unwrap().animation.as_ref().unwrap().clip(), 1);
}
