use crate::{
    app::{Action, App},
    camera::Camera,
    demo,
    model::ModelLibrary,
};
use io_types::Vec3;

#[test]
fn stress_market_ramps_are_connected_walkable_geometry() {
    let library = ModelLibrary::load(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/dungeon/stress-16.json"),
    )
    .unwrap();
    let world = demo::world(&library).unwrap();
    let game = demo::game(&library, &world).unwrap();
    let id = game.traversal().unwrap().navigation_status()[0].0;
    let actor = world.item(id).unwrap();
    let mut position = actor.transform.anchor;
    for dx in [0.2, -0.2] {
        let mut peak = 0_f32;
        for _ in 0..240 {
            let walk = io_world::walk_character(
                &world,
                Some(id),
                position,
                Vec3::new(dx, 0., 0.),
                actor.character_body.unwrap(),
            )
            .unwrap();
            assert!(walk.reached, "ramp blocked at {position:?}: {walk:?}");
            position = walk.position;
            peak = peak.max(position.z);
        }
        assert!(peak > 1.99, "did not reach the deck");
        assert!(position.z.abs() < 0.01, "did not descend: {position:?}");
    }
}

#[test]
fn stress_district_loads_large_population_with_valid_spawns_and_bounded_grid() {
    let library = ModelLibrary::load(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/dungeon/stress-256.json"),
    )
    .unwrap();
    let world = demo::world(&library).unwrap();
    let game = demo::game(&library, &world).unwrap();
    assert_eq!(game.traversal().unwrap().navigation_status().len(), 256);
    for item in world.items().iter().filter(|i| i.character_body.is_some()) {
        let p = item.transform.anchor;
        assert!(
            io_world::character_segment_clear(
                &world,
                Some(item.id),
                p,
                p,
                item.character_body.unwrap()
            ),
            "invalid spawn {} {p:?}",
            item.id
        );
        assert!(
            io_world::character_support(
                &world,
                Some(item.id),
                p,
                item.character_body.unwrap(),
                io_world::SupportProbe::CONTACT
            )
            .unwrap()
            .is_some(),
            "unsupported {}",
            item.id
        );
    }
}

fn guard_library() -> ModelLibrary {
    let mut library = ModelLibrary::load(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/dungeon/guards.json"),
    )
    .unwrap();
    // Accelerated deterministic behavior tests; background scheduling has paced tests.
    library
        .config
        .traversal
        .as_mut()
        .unwrap()
        .navigation
        .as_mut()
        .unwrap()
        .planning = io_traversal::PlanningMode::Inline;
    library
}

#[test]
#[ignore = "Manual release-mode CPU timing probe; not a timing assertion or GPU FPS test"]
fn guards_stair_cpu_probe() {
    for (name, npcs, moving, start) in [
        ("idle-no-ai", false, false, [2., 2., 0.]),
        ("push-stair-no-ai", false, true, [3.64, 2., 0.]),
        ("push-stair-with-ai", true, true, [3.64, 2., 0.]),
    ] {
        let mut library = guard_library();
        let definition = library.config.traversal.as_mut().unwrap();
        definition.barrier_cycle = None;
        if !npcs {
            definition.observations.clear();
            definition.navigation.as_mut().unwrap().agents.clear();
            library
                .config
                .items
                .retain(|i| i.name != "jumper" && i.name != "walker");
        }
        library
            .config
            .items
            .iter_mut()
            .find(|i| i.name == "hero")
            .unwrap()
            .position = start;
        let mut world = demo::world(&library).unwrap();
        let mut game = demo::game(&library, &world).unwrap();
        if moving {
            game.playground_command(
                &mut world,
                io_playground::Command::Move { x: 1., y: 0. }.into(),
            )
            .unwrap();
        }
        let mut ms = Vec::new();
        for _ in 0..600 {
            let start = std::time::Instant::now();
            game.step(&mut world, &[], 1. / 60.).unwrap();
            ms.push(start.elapsed().as_secs_f64() * 1000.);
        }
        ms.sort_by(f64::total_cmp);
        eprintln!(
            "CPU {name}: mean {:.3}ms p95 {:.3}ms max {:.3}ms",
            ms.iter().sum::<f64>() / ms.len() as f64,
            ms[569],
            ms[599]
        );
    }
}

#[test]
fn guards_use_independent_capabilities_to_cross_the_same_course() {
    guards_cross_course(io_traversal::PlanningMode::Inline);
}

#[test]
fn background_guards_execute_stairs_jump_and_bridge_without_penetration() {
    guards_cross_course(io_traversal::PlanningMode::Background);
}

fn guards_cross_course(mode: io_traversal::PlanningMode) {
    let mut library = guard_library();
    let definition = library.config.traversal.as_mut().unwrap();
    definition.navigation.as_mut().unwrap().planning = mode;
    definition.observations.clear();
    definition.barrier_cycle = None;
    for agent in &mut definition.navigation.as_mut().unwrap().agents {
        agent.pursuit = None;
        agent.on_arrival = io_playground::OnArrival::Stop;
    }
    let mut world = demo::world(&library).unwrap();
    let mut game = demo::game(&library, &world).unwrap();
    let ids: Vec<_> = game
        .traversal()
        .unwrap()
        .navigation_status()
        .iter()
        .map(|v| v.0)
        .collect();
    let mut jumped = false;
    let mut bridge = false;
    let mut stairs = [false; 2];
    let mut crossing = Vec3::default();
    // Debug providers can be substantially slower than simulated time, especially
    // while the whole workspace suite runs concurrently. Allow the worker wall time.
    let limit = if mode == io_traversal::PlanningMode::Background {
        60_000
    } else {
        7200
    };
    for _ in 0..limit {
        game.step(&mut world, &[], 1. / 60.).unwrap();
        if mode == io_traversal::PlanningMode::Background {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        for (n, id) in ids.iter().enumerate() {
            let i = world.item(*id).unwrap();
            let p = i.transform.anchor;
            assert!(
                io_world::character_segment_clear(
                    &world,
                    Some(*id),
                    p,
                    p,
                    i.character_body.unwrap()
                ),
                "penetration {id} {p:?}"
            );
            stairs[n] |= p.x > 4. && p.x < 8. && p.z > 0.2;
            if n == 0 {
                jumped |= p.x > 12. && p.x < 13. && p.y < 6. && p.z > 1.5;
            } else {
                if p.x > 12. && p.x < 13. {
                    crossing = p;
                }
                bridge |= p.x > 12. && p.x < 13. && p.y >= 7.999;
                assert!(p.z < 1.05, "non-jumper jumped {p:?}");
            }
        }
        if game
            .traversal()
            .unwrap()
            .navigation_status()
            .iter()
            .all(|v| v.1 == io_traversal::NavigationStatus::Arrived)
        {
            break;
        }
    }
    let diagnostics = game.traversal().unwrap().diagnostics();
    assert!(
        diagnostics
            .iter()
            .all(|d| d.feedback.status == io_traversal::NavigationStatus::Arrived),
        "{diagnostics:?}"
    );
    assert!(
        jumped && bridge && stairs == [true, true],
        "jump {jumped} bridge {bridge} stairs {stairs:?}, crossing {crossing:?}"
    );
}

#[test]
fn guards_remember_elevated_sightings_reacquire_and_eventually_resume_patrol() {
    use io_playground::PursuitPhase;
    let mut library = guard_library();
    let d = library.config.traversal.as_mut().unwrap();
    d.barrier_cycle = None;
    for track in &mut d.observations {
        track.vision.range = 6.;
        track.vision.fov_degrees = 360.;
    }
    for npc in &mut d.navigation.as_mut().unwrap().agents {
        npc.pursuit.as_mut().unwrap().settings.notice_attention = 0.05;
    }
    for i in &mut library.config.items {
        if i.name == "hero" {
            i.position = [10.75, 5., 1.];
        }
        if i.name == "jumper" {
            i.position = [8., 5., 1.];
            i.yaw_degrees = 90.;
        }
    }
    let mut world = demo::world(&library).unwrap();
    let mut game = demo::game(&library, &world).unwrap();
    let hero = game.traversal().unwrap().player();
    for _ in 0..30 {
        game.step(&mut world, &[], 1. / 60.).unwrap();
    }
    let t = game.traversal().unwrap();
    assert_eq!(t.pursuit_status()[0].1, PursuitPhase::Chase);
    assert_eq!(
        t.pursuit_status()[1].1,
        PursuitPhase::Patrol,
        "awareness must not be shared"
    );
    let old_ticket = t.diagnostics()[0].feedback.ticket;
    assert!(world.set_pose(hero, Vec3::new(27., 1., 0.), 0.));
    game.step(&mut world, &[], 1. / 60.).unwrap();
    assert!(game.traversal().unwrap().observations()[0].attention() > 0.);
    assert!(game.traversal().unwrap().observations()[0]
        .contact()
        .is_none());
    for _ in 0..60 {
        game.step(&mut world, &[], 1. / 60.).unwrap();
    }
    let t = game.traversal().unwrap();
    assert_eq!(t.pursuit_status()[0].1, PursuitPhase::Search);
    let debug = &t.diagnostics()[0];
    let Some(io_traversal::NavigationGoal::Position(p)) = debug.goal else {
        panic!("{debug:?}");
    };
    assert!(
        (p.z - 1.).abs() < 0.01 && p.x < 13.,
        "search lost last-seen layer {p:?}"
    );
    assert!(debug.feedback.ticket.revision > old_ticket.revision);
    let before_reacquire = debug.feedback.ticket;
    assert!(world.set_pose(hero, Vec3::new(10.75, 3.8, 1.), 0.));
    for _ in 0..60 {
        game.step(&mut world, &[], 1. / 60.).unwrap();
    }
    let t = game.traversal().unwrap();
    assert!(matches!(
        t.pursuit_status()[0].1,
        PursuitPhase::Chase | PursuitPhase::Tagged
    ));
    assert!(t.diagnostics()[0].feedback.ticket.revision > before_reacquire.revision);
    assert!(world.set_pose(hero, Vec3::new(27., 1., 0.), 0.));
    let mut returned = false;
    for _ in 0..1500 {
        game.step(&mut world, &[], 1. / 60.).unwrap();
        if game.traversal().unwrap().pursuit_status()[0].1 == PursuitPhase::Patrol {
            returned = true;
            break;
        }
    }
    assert!(returned);
}

#[test]
fn guards_replan_when_a_blocked_bridge_reopens() {
    let mut library = guard_library();
    let d = library.config.traversal.as_mut().unwrap();
    d.observations.clear();
    d.barrier_cycle = None;
    // One guard isolates topology invalidation from transient actor avoidance.
    d.navigation
        .as_mut()
        .unwrap()
        .agents
        .retain(|a| a.item == "walker");
    d.navigation.as_mut().unwrap().agents[0].pursuit = None;
    d.navigation.as_mut().unwrap().agents[0].on_arrival = io_playground::OnArrival::Stop;
    library.config.items.retain(|i| i.name != "jumper");
    let shutter = library
        .config
        .items
        .iter()
        .position(|i| i.name == "shutter")
        .unwrap();
    library.config.items[shutter].position[2] = 1.;
    let mut world = demo::world(&library).unwrap();
    let barrier = world.items()[shutter].id;
    let mut game = demo::game(&library, &world).unwrap();
    for _ in 0..900 {
        game.step(&mut world, &[], 1. / 60.).unwrap();
    }
    assert_eq!(
        game.traversal().unwrap().navigation_status()[0].1,
        io_traversal::NavigationStatus::Unreachable
    );
    let revision = world.navigation_revision();
    assert!(world.set_kinematic_target(
        barrier,
        Vec3::new(12., 8., 5.),
        io_types::Rotation::default()
    ));
    for _ in 0..6000 {
        game.step(&mut world, &[], 1. / 60.).unwrap();
        if game.traversal().unwrap().navigation_status()[0].1
            == io_traversal::NavigationStatus::Arrived
        {
            break;
        }
    }
    assert!(world.navigation_revision() > revision);
    assert_eq!(
        game.traversal().unwrap().navigation_status()[0].1,
        io_traversal::NavigationStatus::Arrived
    );
}

#[test]
fn guards_barrier_defers_closing_when_player_occupies_it() {
    let mut library = guard_library();
    let d = library.config.traversal.as_mut().unwrap();
    d.barrier_cycle.as_mut().unwrap().interval_seconds = 2.;
    d.observations.clear();
    d.navigation.as_mut().unwrap().agents.clear();
    let shutter = library
        .config
        .items
        .iter()
        .position(|i| i.name == "shutter")
        .unwrap();
    let mut world = demo::world(&library).unwrap();
    let barrier = world.items()[shutter].id;
    let mut game = demo::game(&library, &world).unwrap();
    let hero = game.traversal().unwrap().player();
    assert!(world.set_pose(hero, Vec3::new(12.5, 9., 1.), 0.));
    for _ in 0..180 {
        game.step(&mut world, &[], 1. / 60.).unwrap();
    }
    assert!((world.item(barrier).unwrap().transform.anchor.z - 5.).abs() < 0.01);
    assert!(world.set_pose(hero, Vec3::new(25., 2., 0.), 0.));
    for _ in 0..3 {
        game.step(&mut world, &[], 1. / 60.).unwrap();
    }
    assert!(
        (world.item(barrier).unwrap().transform.anchor.z - 1.).abs() < 0.01,
        "barrier {:?}",
        world.item(barrier).unwrap()
    );
}

fn athletics_demo() -> (ModelLibrary, io_world::World, crate::game::Game) {
    let mut library = ModelLibrary::load(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/dungeon/athletics.json"),
    )
    .unwrap();
    library
        .config
        .traversal
        .as_mut()
        .unwrap()
        .navigation
        .as_mut()
        .unwrap()
        .planning = io_traversal::PlanningMode::Inline;
    let world = demo::world(&library).unwrap();
    let game = demo::game(&library, &world).unwrap();
    (library, world, game)
}

#[test]
fn athletics_npc_discovers_stairs_jump_and_detour_in_both_directions() {
    let (_, mut world, mut game) = athletics_demo();
    let id = game.traversal().unwrap().navigation_status()[0].0;
    let mut east = false;
    let mut airborne = false;
    let mut stair_up = false;
    let mut stair_down = false;
    let mut detour = false;
    let mut returned = false;
    for _ in 0..3600 {
        game.step(&mut world, &[], 1. / 60.).unwrap();
        let item = world.item(id).unwrap();
        let p = item.transform.anchor;
        assert!(
            io_world::character_segment_clear(&world, Some(id), p, p, item.character_body.unwrap()),
            "penetration {p:?}"
        );
        if p.x > 25.9 {
            east = true;
        }
        airborne |= p.x > 12. && p.x < 13. && p.z > 1.5;
        let feedback = game.traversal().unwrap().athletics_feedback().unwrap()[0];
        stair_up |= p.x > 4.
            && p.x < 8.
            && p.z > 0.2
            && p.z < 0.9
            && feedback.motion == io_playground::Motion::Walk;
        stair_down |= p.x > 17.
            && p.x < 20.
            && p.z > 0.2
            && p.z < 0.9
            && feedback.motion == io_playground::Motion::Walk;
        detour |= p.x > 22. && p.x < 23. && p.y > 4.3;
        if east && p.x < 2.1 {
            returned = true;
            break;
        }
    }
    assert!(east && returned && airborne && stair_up && stair_down && detour,
        "east={east} returned={returned} airborne={airborne} stairs={stair_up}/{stair_down} detour={detour}, final={:?}, status={:?}",
        world.item(id).unwrap().transform.anchor,game.traversal().unwrap().navigation_status());
}

fn navigation_demo() -> (ModelLibrary, io_world::World, crate::game::Game) {
    let library = ModelLibrary::load(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/dungeon/navigation.json"),
    )
    .unwrap();
    let world = demo::world(&library).unwrap();
    let game = demo::game(&library, &world).unwrap();
    (library, world, game)
}

#[test]
fn navigation_scout_enters_passage_and_chamber_without_rendering() {
    use io_traversal::NavigationStatus;
    let (_, mut world, mut game) = navigation_demo();
    let id = game.traversal().unwrap().navigation_status()[0].0;
    let mut crouched = false;
    for _ in 0..2400 {
        // No render-visible items: plugin activation owns this simulation.
        game.step(&mut world, &[], 1. / 60.).unwrap();
        crouched |= world.item(id).unwrap().character_body.unwrap().height < 1.5;
        if game.traversal().unwrap().navigation_status()[0].1 == NavigationStatus::Arrived {
            break;
        }
    }
    let status = game.traversal().unwrap().navigation_status();
    let actor = world.item(id).unwrap();
    assert_eq!(
        status[0].1,
        NavigationStatus::Arrived,
        "position {:?}, status {status:?}",
        actor.transform.anchor
    );
    assert!(crouched);
    assert!(actor.transform.anchor.y < 37.02);
    assert!(actor.character_body.unwrap().height > 1.5);
    assert_eq!(
        world.space_location(id),
        Some(io_world::SpaceLocation::Interior(io_world::InteriorId(1)))
    );
    assert!(actor.simulated_ticks > 300);
    assert!(status[0].2 > 2);
}

#[test]
fn navigation_rejects_impossible_capability_without_pushing_into_walls() {
    use io_traversal::NavigationStatus;
    let (mut library, mut world, _) = navigation_demo();
    library
        .config
        .traversal
        .as_mut()
        .unwrap()
        .navigation
        .as_mut()
        .unwrap()
        .agents[0]
        .can_crouch = false;
    let mut game = demo::game(&library, &world).unwrap();
    let id = game.traversal().unwrap().navigation_status()[0].0;
    let start = world.item(id).unwrap().transform.anchor;
    for _ in 0..600 {
        game.step(&mut world, &[], 1. / 60.).unwrap();
    }
    assert_eq!(
        game.traversal().unwrap().navigation_status()[0].1,
        NavigationStatus::Unreachable
    );
    let end = world.item(id).unwrap().transform.anchor;
    assert!((start - end).dot(start - end) < 0.0001);
}

#[test]
fn navigation_config_rejects_typos_and_unresolved_actors() {
    let text = include_str!("../assets/dungeon/navigation.json");
    assert!(serde_json::from_str::<crate::config::SceneConfig>(
        &text.replace("can_crouch", "can_crouhc")
    )
    .is_err());
    assert!(serde_json::from_str::<crate::config::SceneConfig>(
        &text.replace("walk_speed", "walk_spead")
    )
    .is_err());
    let (mut library, world, _) = navigation_demo();
    library
        .config
        .traversal
        .as_mut()
        .unwrap()
        .navigation
        .as_mut()
        .unwrap()
        .agents[0]
        .item = "missing".into();
    assert!(demo::game(&library, &world)
        .unwrap_err()
        .contains("unknown traversal actor"));
}

#[test]
fn navigation_approaches_an_occupied_goal_and_recovers_without_geometry_invalidation() {
    use io_traversal::NavigationStatus;
    let (_, mut world, mut game) = navigation_demo();
    let player = game.traversal().unwrap().player();
    assert!(world.set_pose(player, Vec3::new(50., 37., 0.), 0.));
    let revision = world.navigation_revision();
    for _ in 0..300 {
        game.step(&mut world, &[], 1. / 60.).unwrap();
    }
    assert_eq!(
        game.traversal().unwrap().navigation_status()[0].1,
        NavigationStatus::Following
    );
    assert!(world.set_pose(player, Vec3::new(54., 60., 0.), 0.));
    assert_eq!(revision, world.navigation_revision());
    for _ in 0..3000 {
        game.step(&mut world, &[], 1. / 60.).unwrap();
        if game.traversal().unwrap().navigation_status()[0].1 == NavigationStatus::Arrived {
            break;
        }
    }
    assert_eq!(
        game.traversal().unwrap().navigation_status()[0].1,
        NavigationStatus::Arrived
    );
}

#[test]
fn navigation_planning_budget_is_shared_and_fair_between_agents() {
    let (mut library, _, _) = navigation_demo();
    let mut data: serde_json::Value =
        serde_json::from_str(include_str!("../assets/dungeon/navigation.json")).unwrap();
    let mut npc = data["items"].as_array().unwrap().last().unwrap().clone();
    npc["name"] = "second".into();
    npc["position"] = serde_json::json!([52, 60, 0]);
    data["items"].as_array_mut().unwrap().push(npc);
    data["traversal"]["navigation"]["agents"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "item":"second", "goal":[52,38,0]
        }));
    library.config = serde_json::from_value(data).unwrap();
    let mut world = demo::world(&library).unwrap();
    let mut game = demo::game(&library, &world).unwrap();
    for _ in 0..20 {
        let before = game
            .traversal()
            .unwrap()
            .navigation_stats()
            .unwrap()
            .expanded;
        game.step(&mut world, &[], 1. / 60.).unwrap();
        let after = game
            .traversal()
            .unwrap()
            .navigation_stats()
            .unwrap()
            .expanded;
        assert!(after - before <= 12);
    }
    let status = game.traversal().unwrap().navigation_status();
    assert_eq!(status.len(), 2);
    assert!(status
        .iter()
        .all(|(id, _, known)| *known > 0 && world.item(*id).unwrap().simulated_ticks == 20));
}

fn app() -> App {
    let library = Box::leak(Box::new(
        ModelLibrary::load(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/dungeon/entry.json"),
        )
        .unwrap(),
    ));
    let world = demo::world(library).unwrap();
    let game = demo::game(library, &world).unwrap();
    let player = game.traversal().unwrap().player();
    let index = world.items().iter().position(|i| i.id == player).unwrap();
    let mut camera =
        Camera::new(world.item(player).unwrap().transform.anchor + Vec3::new(0., 0., 1.));
    camera.set_zoom(library.config.camera.zoom);
    camera.set_projection(library.config.camera.projection);
    camera.set_orbit_camera(library.config.camera.orbit);
    camera.set_interior_camera(library.config.camera.interior);
    let mut app = App::with_world(world, camera, Some(index), library);
    app.set_test_game(game);
    app
}
#[test]
#[ignore = "Known overhead clearance regression; user deferred overhead behavior while fixing wall sliding"]
fn stationary_overhead_orbit_never_dives_toward_actor_in_dungeon() {
    let a = app();
    let player = a.game().traversal().unwrap().player();
    for target in [
        Vec3::new(50., 58., 1.33),
        Vec3::new(50., 46., 0.805),
        Vec3::new(50., 37., 1.33),
    ] {
        for yaw in [0., 0.7, 1.8, 3.] {
            let mut camera = a.camera(1).unwrap().clone();
            camera.set_target(target);
            let (current_yaw, current_pitch, _) = camera.capture_rig();
            camera.orbit(yaw - current_yaw.to_radians(), -current_pitch.to_radians());
            camera.resolve_orbit(a.world(), Vec3::default(), Some(player), 0.);
            let initial = 1. / camera.view().convergence;
            for _ in 0..240 {
                camera.orbit_around(a.world(), Some(player), 0., 0.02);
                camera.resolve_orbit(a.world(), Vec3::default(), Some(player), 1. / 60.);
                let distance = 1. / camera.view().convergence;
                assert!(
                    distance >= initial - 0.003,
                    "overhead dive at {target:?}/{yaw}: {initial} -> {distance}"
                );
            }
        }
    }
}
#[test]
fn jogging_and_abdomen_pivot_follow_the_existing_character_components() {
    let mut a = app();
    a.update(0.25);
    let player = a.game().traversal().unwrap().player();
    let start = a.world().item(player).unwrap().transform.anchor;
    assert!((a.camera(1).unwrap().target().z - start.z - 1.33).abs() < 1e-4);
    let (right, _, _) = a.camera(1).unwrap().basis();
    a.game_action(2, 0, right.y, -right.x);
    for _ in 0..60 {
        a.update(1. / 60.);
    }
    let item = a.world().item(player).unwrap();
    assert!((item.transform.anchor.y - start.y - 3.4).abs() < 0.01);
    a.game_action(2, 0, 0., 0.);
    a.game_action(12, 1, 0., 0.);
    a.update(0.25);
    let item = a.world().item(player).unwrap();
    assert!((a.camera(1).unwrap().target().z - item.transform.anchor.z - 0.805).abs() < 1e-4);
}
#[test]
fn dungeon_orbit_retains_angles_lens_and_zoom_intent_across_interiors() {
    let mut a = app();
    let player = a.game().traversal().unwrap().player();
    let move_ticks = |a: &mut App, n, forward: f32| {
        for _ in 0..n {
            let camera = a.camera(1).unwrap();
            let (right, _, _) = camera.basis();
            assert!(a.game_action(2, 0, -right.y * forward, right.x * forward));
            a.update(1. / 60.);
        }
    };
    move_ticks(&mut a, 300, 1.);
    assert!(a.game_action(12, 1, 0., 0.));
    move_ticks(&mut a, 260, 1.);
    assert!(a.game_action(2, 0, 0., 0.));
    for _ in 0..120 {
        a.update(1. / 60.);
    }
    let camera = a.camera(1).unwrap();
    let view = camera.view();
    let distance = 1. / view.convergence;
    let (_, pitch, _) = camera.capture_rig();
    assert!((pitch - 35.26439).abs() < 0.001);
    assert!((2. * (view.half_height * view.convergence).atan().to_degrees() - 45.).abs() < 0.001);
    let before = camera.basis().2;
    assert!(a.dispatch(Action::Orbit {
        yaw: 0.03,
        pitch: 0.
    }));
    a.update(1. / 60.);
    let after = a.camera(1).unwrap().basis().2;
    let (by, _) = crate::camera_steering::angles(before);
    let (ay, _) = crate::camera_steering::angles(after);
    assert!((ay - by - 0.03).abs() < 0.001);
    assert!(a.dispatch(Action::Zoom { steps: -20. }));
    for _ in 0..120 {
        a.update(1. / 60.);
    }
    let view = a.camera(1).unwrap().view();
    assert!(
        (1. / view.convergence - distance).abs() < 0.1,
        "zoom-out is blocked by the ceiling, not converted to FOV or elevation"
    );
    let eye = view.target + view.forward.scaled(1. / view.convergence);
    assert_eq!(
        io_world::cast_sphere(a.world(), view.target, eye, 0.12, Some(player)).unwrap(),
        1.
    );
    move_ticks(&mut a, 800, -1.);
    assert!(a.world().item(player).unwrap().transform.anchor.y > 56.);
    assert!(a.game_action(2, 0, 0., 0.));
    assert!(a.game_action(12, 0, 0., 0.));
    for _ in 0..120 {
        a.update(1. / 60.);
    }
    assert!(
        1. / a.camera(1).unwrap().view().convergence > 20.,
        "exit should recover the preferred distance"
    );
}
#[test]
fn dungeon_controls_crossfade_jump_crouch_and_retain_worker_contracts() {
    let mut a = app();
    let id = a.game().traversal().unwrap().player();
    a.update(0.25);
    let first = a.frame(1).unwrap().joint_matrices.clone();
    assert!(a.game_action(2, 0, 0., 1.));
    for _ in 0..30 {
        a.update(1. / 60.);
    }
    assert_ne!(a.frame(1).unwrap().joint_matrices, first);
    assert!(a.game_action(2, 0, 0., 0.));
    assert!(a.game_action(4, 0, 0., 0.));
    a.update(0.2);
    assert!(a.world().item(id).unwrap().transform.anchor.z > 0.3);
    for _ in 0..80 {
        a.update(1. / 60.);
    }
    assert!(a.game_action(12, 1, 0., 0.));
    a.update(0.25);
    assert!(a.game().traversal().unwrap().crouched());
    assert!(a.dispatch(Action::Orbit {
        yaw: 0.3,
        pitch: 0.1
    }));
    assert!(!a.game_action(12, 2, 0., 0.));
    let mut a = a.into_threaded().unwrap();
    assert!(a.game_action(12, 0, 0., 0.));
    for _ in 0..100 {
        std::thread::sleep(std::time::Duration::from_millis(5));
        a.update(0.005);
        if !a.game().traversal().unwrap().crouched() {
            return;
        }
    }
    panic!("worker did not publish stance release");
}
