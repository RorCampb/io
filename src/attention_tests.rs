use crate::{demo, model::ModelLibrary};
use io_types::Vec3;

fn library() -> ModelLibrary {
    ModelLibrary::load(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/dungeon/attention.json"),
    )
    .unwrap()
}

fn reaction_library() -> ModelLibrary {
    ModelLibrary::load(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/dungeon/reactions.json"),
    )
    .unwrap()
}

#[test]
fn discovered_player_attention_ramps_up_then_decays_outside_view() {
    let mut library = reaction_library();
    let traversal = library.config.traversal.as_mut().unwrap();
    traversal
        .obstacle_observations
        .as_mut()
        .unwrap()
        .character_tracks_per_observer = 6;
    let npc = &mut traversal.navigation.as_mut().unwrap().agents[0];
    npc.goal = [20., 40., 0.];
    npc.on_arrival = io_playground::OnArrival::Stop;
    let mut world = demo::world(&library).unwrap();
    let mut game = demo::game(&library, &world).unwrap();
    let player = game.traversal().unwrap().player();
    let observer = game.traversal().unwrap().navigation_status()[0].0;
    assert!(world.set_pose_3d(player, Vec3::new(20., 37., 0.), Default::default()));
    for _ in 0..120 {
        game.step(&mut world, &[], 1. / 60.).unwrap();
    }
    let t = game.traversal().unwrap();
    let before = t
        .discovered_observations()
        .find(|o| o.observer() == observer && o.target() == player)
        .unwrap()
        .attention();
    assert!(before > 0.15);
    assert!(t.observers_noticing(player) > 0);
    assert!(world.set_pose_3d(player, Vec3::new(20., 47., 0.), Default::default()));
    for _ in 0..60 {
        game.step(&mut world, &[], 1. / 60.).unwrap();
    }
    let after = game
        .traversal()
        .unwrap()
        .discovered_observations()
        .find(|o| o.observer() == observer && o.target() == player)
        .unwrap();
    assert_eq!(after.evidence(), 0.);
    assert!(after.attention() < before);
}

#[test]
fn crossing_demo_predicts_motion_and_retains_collision_safe_execution() {
    for hz in [30, 60, 144] {
        let mut library = reaction_library();
        library
            .config
            .traversal
            .as_mut()
            .unwrap()
            .navigation
            .as_mut()
            .unwrap()
            .planning = io_traversal::PlanningMode::Inline;
        let mut world = demo::world(&library).unwrap();
        let mut game = demo::game(&library, &world).unwrap();
        let mut first_reaction = None;
        for tick in 0..hz * 12 {
            game.step(&mut world, &[], 1. / hz as f32).unwrap();
            let t = game.traversal().unwrap();
            if t.discovery_stats().unwrap().predictive_replans > 0 {
                first_reaction.get_or_insert(tick as f32 / hz as f32);
            }
            for npc in t.diagnostics() {
                let item = world.item(npc.actor).unwrap();
                assert!(io_world::character_fits(
                    &world,
                    npc.actor,
                    item.transform.anchor,
                    item.character_body.unwrap()
                ));
            }
        }
        let stats = game.traversal().unwrap().discovery_stats().unwrap();
        eprintln!("crossing {hz}Hz first={first_reaction:?} stats={stats:?}");
        assert!(first_reaction.is_some_and(|t| t < 5.));
        // Successful early replanning may avoid reaching the urgent/braking threshold.
        // Urgent prediction and braking are tested independently with forced conflicts.
        assert!(stats.predictive_replans > 0);
        assert!(
            stats.predictive_replans < 100,
            "bounded replan cadence: {stats:?}"
        );
    }
}

#[test]
fn npc_meters_project_world_width_and_shrink_with_zoom() {
    let library = Box::leak(Box::new(reaction_library()));
    let world = demo::world(library).unwrap();
    let game = demo::game(library, &world).unwrap();
    let mut camera = crate::camera::Camera::new(Vec3::new(100., 100., 30.));
    camera.set_zoom(1.);
    let mut state = crate::app::App::with_world(world, camera, None, library);
    state.set_test_game(game);
    let mut app = crate::IoApp { state };
    let mut view = std::mem::MaybeUninit::<crate::IoGameView>::uninit();
    assert!(unsafe { crate::io_app_game_view(&app, view.as_mut_ptr()) });
    let before = unsafe { view.assume_init() };
    assert_eq!(before.meter_count, 6);
    assert!(before.meters[..6]
        .iter()
        .all(|m| m.width > 5. && m.width.is_finite()));
    assert_eq!(before.meters[0].label[0], b'E');
    assert_eq!(before.meters[1].label[0], b'A');
    assert!(app.state.dispatch(crate::app::Action::Zoom { steps: -3. }));
    app.state.update(0.1);
    let mut view = std::mem::MaybeUninit::<crate::IoGameView>::uninit();
    assert!(unsafe { crate::io_app_game_view(&app, view.as_mut_ptr()) });
    let after = unsafe { view.assume_init() };
    assert_eq!(after.meter_count, 6);
    assert!(after.meters[0].width < before.meters[0].width);
    assert!((after.meters[1].y - after.meters[0].y - after.meters[0].width * 0.24).abs() < 0.001);
}
#[test]
fn attention_demo_npc_observes_player_offscreen_and_retains_awareness_when_sight_breaks() {
    let library = library();
    let mut world = demo::world(&library).unwrap();
    let mut game = demo::game(&library, &world).unwrap();
    let track = &game.traversal().unwrap().observations()[0];
    let target = track.target();
    let observer = track.observer();
    assert_eq!(track.label(), "runner");
    assert_eq!(target, game.traversal().unwrap().player());
    let (mut hidden, mut partial, mut clear, mut decayed) = (false, false, false, false);
    let mut last = 0.;
    let mut peak = 0_f32;
    let mut peak_focus = 0_f32;
    for _ in 0..1800 {
        game.step(&mut world, &[], 1. / 60.).unwrap();
        let track = &game.traversal().unwrap().observations()[0];
        hidden |= track.exposure() == 0.;
        partial |= track.exposure() > 0. && track.exposure() < 0.99;
        clear |= track.exposure() > 0.99;
        decayed |= track.exposure() == 0. && track.attention() < last && track.attention() > 0.1;
        peak = peak.max(track.attention());
        peak_focus = peak_focus.max(track.evidence());
        assert!(track.attention() <= peak_focus + 1e-6);
        last = track.attention();
    }
    assert!(
        hidden && partial && clear && decayed,
        "hidden={hidden}, partial={partial}, clear={clear}, decayed={decayed}"
    );
    assert!(
        peak > 0.5 && peak <= peak_focus,
        "peak={peak}, focus={peak_focus}"
    );
    assert_eq!(world.item(target).unwrap().simulated_ticks, 1800);
    assert_eq!(world.item(observer).unwrap().simulated_ticks, 1800);
}
#[test]
fn attention_contract_rejects_unknown_actors_bad_settings_and_unbound_tracks() {
    let mut library = library();
    let world = demo::world(&library).unwrap();
    library.config.traversal.as_mut().unwrap().observations[0].target = "missing".into();
    assert!(demo::game(&library, &world)
        .unwrap_err()
        .contains("unknown observation actor"));
    let text = include_str!("../assets/dungeon/attention.json");
    assert!(serde_json::from_str::<crate::config::SceneConfig>(
        &text.replace("gain_per_second", "gain_per_secod")
    )
    .is_err());
    let mut config: crate::config::SceneConfig = serde_json::from_str(text).unwrap();
    config.traversal.as_mut().unwrap().observations[0]
        .vision
        .fov_degrees = 361.;
    assert!(config.traversal.unwrap().validate().is_err());
}
#[test]
fn attention_label_anchor_uses_interpolated_target_pose_not_observer_pose() {
    let library = Box::leak(Box::new(library()));
    let world = demo::world(library).unwrap();
    let game = demo::game(library, &world).unwrap();
    let target = game.traversal().unwrap().observations()[0].target();
    let mut app = crate::app::App::with_world(
        world,
        crate::camera::Camera::new(Vec3::new(50., 54., 1.)),
        None,
        library,
    );
    app.set_test_game(game);
    let before = app.item_label_anchor(target).unwrap();
    assert!(app.game_action(2, 0, 1., 0.));
    app.update(0.1);
    let after = app.item_label_anchor(target).unwrap();
    assert!((after - before).dot(after - before) > 0.);
    assert!((after.z - 2.15).abs() < 0.01);
    assert!(app.item_label_anchor(u64::MAX).is_none());
}

fn cat_mouse_library() -> ModelLibrary {
    ModelLibrary::load(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/dungeon/cat-mouse.json"),
    )
    .unwrap()
}

#[test]
fn live_attention_tuning_is_validated_atomic_and_keeps_memory() {
    use io_playground::Input;
    let library = cat_mouse_library();
    let mut world = demo::world(&library).unwrap();
    let mut game = demo::game(&library, &world).unwrap();
    for _ in 0..120 {
        game.step(&mut world, &[], 1. / 60.).unwrap();
    }
    let t = game.traversal().unwrap();
    let (mut settings, has_pursuit) = t.attention_settings(0).unwrap();
    assert!(has_pursuit);
    let attention = t.observations()[0].attention();
    let phase = t.pursuit_status();
    settings.fov_degrees = 300.;
    settings.notice_attention = 0.7;
    game.playground_command(&mut world, Input::TuneAttention(settings))
        .unwrap();
    let t = game.traversal().unwrap();
    assert_eq!(t.observations()[0].attention(), attention);
    assert_eq!(t.pursuit_status(), phase);
    assert_eq!(t.attention_settings(0).unwrap().0.notice_attention, 0.7);
    for bad in [
        io_playground::AttentionSettings {
            fov_degrees: f32::NAN,
            ..settings
        },
        io_playground::AttentionSettings {
            notice_attention: 0.,
            ..settings
        },
        io_playground::AttentionSettings {
            target: u64::MAX,
            ..settings
        },
    ] {
        assert!(game
            .playground_command(&mut world, Input::TuneAttention(bad))
            .is_err());
        let current = game.traversal().unwrap().attention_settings(0).unwrap().0;
        assert_eq!(current.fov_degrees, 300.);
        assert_eq!(current.notice_attention, 0.7);
    }
}

#[test]
fn worker_applies_attention_controls_and_publishes_settings() {
    use crate::worker::{Event, Worker};
    use std::time::{Duration, Instant};
    let library = cat_mouse_library();
    let world = demo::world(&library).unwrap();
    let game = demo::game(&library, &world).unwrap();
    let mut settings = game.traversal().unwrap().attention_settings(0).unwrap().0;
    settings.fov_degrees = 280.;
    settings.notice_attention = 0.25;
    let mut worker = Worker::spawn_with_game(world, vec![], Default::default(), game).unwrap();
    let id = worker
        .submit_playground(io_playground::Input::TuneAttention(settings))
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut acknowledged = false;
    loop {
        worker.poll_snapshot();
        while let Some(reply) = worker.poll_event() {
            if reply.id == id {
                assert!(matches!(
                    reply.payload,
                    Event::TraversalCompleted {
                        outcome: Ok(()),
                        ..
                    }
                ));
                acknowledged = true;
            }
        }
        let applied = worker
            .current
            .game
            .traversal()
            .unwrap()
            .attention_settings(0)
            .unwrap()
            .0;
        if acknowledged && applied.fov_degrees == 280. {
            assert_eq!(applied.notice_attention, 0.25);
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn cat_mouse_npc_patrols_acquires_and_tags_stationary_player_offscreen() {
    use io_playground::PursuitPhase;
    let library = cat_mouse_library();
    let mut world = demo::world(&library).unwrap();
    let mut game = demo::game(&library, &world).unwrap();
    let npc = game.traversal().unwrap().pursuit_status()[0].0;
    let initial = world.item(npc).unwrap().transform.anchor;
    let mut chased = false;
    let mut tagged = false;
    for _ in 0..2400 {
        game.step(&mut world, &[], 1. / 60.).unwrap();
        let (_, phase, tags) = game.traversal().unwrap().pursuit_status()[0];
        chased |= phase == PursuitPhase::Chase;
        if phase == PursuitPhase::Tagged {
            tagged = true;
            assert!(tags > 0);
            break;
        }
    }
    assert!(
        chased && tagged,
        "chased={chased}, tagged={tagged}, status={:?}, nav={:?}, position={:?}",
        game.traversal().unwrap().pursuit_status(),
        game.traversal().unwrap().navigation_status(),
        world.item(npc).unwrap().transform.anchor
    );
    let end = world.item(npc).unwrap().transform.anchor;
    assert!((end - initial).dot(end - initial) > 1.);
    let item = world.item(npc).unwrap();
    assert!(io_world::character_fits(
        &world,
        npc,
        end,
        item.character_body.unwrap()
    ));
}

#[test]
fn cat_mouse_hidden_player_does_not_supply_new_sightings_and_search_expires() {
    use io_playground::PursuitPhase;
    let library = cat_mouse_library();
    let mut world = demo::world(&library).unwrap();
    let mut game = demo::game(&library, &world).unwrap();
    let hero = game.traversal().unwrap().player();
    for _ in 0..1200 {
        game.step(&mut world, &[], 1. / 60.).unwrap();
        if game.traversal().unwrap().pursuit_status()[0].1 == PursuitPhase::Chase {
            break;
        }
    }
    assert_eq!(
        game.traversal().unwrap().pursuit_status()[0].1,
        PursuitPhase::Chase
    );
    // Test-only relocation outside the enclosing wall. The policy must not follow this pose.
    world.set_pose(hero, Vec3::new(65., 65., 0.), 0.);
    let mut searched = false;
    let mut returned = false;
    for _ in 0..1000 {
        game.step(&mut world, &[], 1. / 60.).unwrap();
        let t = game.traversal().unwrap();
        assert_eq!(t.observations()[0].evidence(), 0.);
        let (_, phase, tags) = t.pursuit_status()[0];
        searched |= phase == PursuitPhase::Search;
        returned |= searched && phase == PursuitPhase::Patrol;
        assert_eq!(tags, 0);
    }
    assert!(searched && returned);
}

#[test]
fn cat_mouse_contract_requires_matching_vision_and_valid_policy() {
    let mut library = cat_mouse_library();
    let def = library.config.traversal.as_mut().unwrap();
    def.observations.clear();
    assert!(def
        .validate()
        .unwrap_err()
        .contains("matching observer/target"));
    let mut library = cat_mouse_library();
    let def = library.config.traversal.as_mut().unwrap();
    def.navigation.as_mut().unwrap().agents[0]
        .pursuit
        .as_mut()
        .unwrap()
        .settings
        .search_seconds = f32::NAN;
    assert!(def.validate().is_err());
    assert!(
        serde_json::from_str::<io_locomotion::SteeringSettings>(r#"{"accelleration":5}"#).is_err()
    );
    let mut library = cat_mouse_library();
    let def = library.config.traversal.as_mut().unwrap();
    def.navigation.as_mut().unwrap().agents[0]
        .steering
        .acceleration = f32::NAN;
    assert!(def.validate().is_err());
}

#[test]
fn cat_mouse_repaths_to_a_moving_visible_player_without_starving_navigation() {
    use io_playground::{Command, PursuitPhase};
    let library = cat_mouse_library();
    let mut world = demo::world(&library).unwrap();
    let mut game = demo::game(&library, &world).unwrap();
    for _ in 0..1200 {
        game.step(&mut world, &[], 1. / 60.).unwrap();
        if game.traversal().unwrap().pursuit_status()[0].1 == PursuitPhase::Chase {
            break;
        }
    }
    let hero = game.traversal().unwrap().player();
    let before = world.item(hero).unwrap().transform.anchor;
    assert_eq!(
        game.traversal().unwrap().pursuit_status()[0].1,
        PursuitPhase::Chase
    );
    game.traversal_command(&mut world, Command::Move { x: -0.25, y: 0. })
        .unwrap();
    let mut tagged = false;
    let mut trace = Vec::new();
    for tick in 0..600 {
        game.step(&mut world, &[], 1. / 60.).unwrap();
        if tick % 60 == 0 {
            let t = game.traversal().unwrap();
            let observer = t.observations()[0].observer();
            trace.push((
                tick,
                world.item(hero).unwrap().transform.anchor,
                world.item(observer).unwrap().transform.anchor,
                t.observations()[0].evidence(),
                t.pursuit_status(),
                t.navigation_status(),
            ));
        }
        if game.traversal().unwrap().pursuit_status()[0].1 == PursuitPhase::Tagged {
            tagged = true;
            break;
        }
    }
    let after = world.item(hero).unwrap().transform.anchor;
    assert!((after - before).dot(after - before) > 1.);
    assert!(
        tagged,
        "status={:?}, nav={:?}, trace={trace:?}",
        game.traversal().unwrap().pursuit_status(),
        game.traversal().unwrap().navigation_status()
    );
}

#[test]
fn cat_mouse_routes_around_low_cover_without_waiting_for_target_to_crouch() {
    use io_playground::{OnArrival, PursuitPhase};
    let mut library = cat_mouse_library();
    library
        .config
        .items
        .iter_mut()
        .find(|i| i.name == "hero")
        .unwrap()
        .position = [55., 52.5, 0.];
    library
        .config
        .items
        .iter_mut()
        .find(|i| i.name == "runner")
        .unwrap()
        .position = [55., 57., 0.];
    let npc = &mut library
        .config
        .traversal
        .as_mut()
        .unwrap()
        .navigation
        .as_mut()
        .unwrap()
        .agents[0];
    npc.goal = [55., 56., 0.];
    npc.on_arrival = OnArrival::Stop;
    let mut world = demo::world(&library).unwrap();
    let mut game = demo::game(&library, &world).unwrap();
    let mut tagged = false;
    let actor = game.traversal().unwrap().pursuit_status()[0].0;
    let mut detour = 0_f32;
    for _ in 0..900 {
        game.step(&mut world, &[], 1. / 60.).unwrap();
        let item = world.item(actor).unwrap();
        detour = detour.max((item.transform.anchor.x - 55.).abs());
        assert!(io_world::character_fits(
            &world,
            actor,
            item.transform.anchor,
            item.character_body.unwrap()
        ));
        if game.traversal().unwrap().pursuit_status()[0].1 == PursuitPhase::Tagged {
            tagged = true;
            break;
        }
    }
    let t = game.traversal().unwrap();
    assert!(
        tagged,
        "phase={:?}, nav={:?}, focus={}",
        t.pursuit_status(),
        t.navigation_status(),
        t.observations()[0].evidence()
    );
    assert!(
        detour > 1.3,
        "expected a real detour around the 2m-wide barrier, got {detour}"
    );
}
