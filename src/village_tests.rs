use crate::{demo, game::Game, model::ModelLibrary};
use io_encounter::{CombatEvent, Encounter, GameCommand, GameError, Phase};
use io_types::Vec3;
use io_world::World;
use std::{collections::BTreeMap, path::Path};

fn fixture() -> (Game, World, BTreeMap<String, u64>) {
    let library = ModelLibrary::load(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/villages/world.json"),
    )
    .unwrap();
    let world = demo::world(&library).unwrap();
    let game = demo::game(&library, &world).unwrap();
    let ids = library
        .config
        .items
        .iter()
        .enumerate()
        .map(|(i, v)| (v.name.clone(), i as u64 + 1))
        .collect();
    (game, world, ids)
}
fn tick(game: &mut Game, world: &mut World, seconds: f32) {
    for _ in 0..(seconds * 4.) as usize {
        game.step(world, &[], 0.25).unwrap();
    }
}

#[test]
fn zoomed_out_village_frame_keeps_every_visible_terrain_tile() {
    use crate::{camera::Camera, projection::Frame};
    let library = ModelLibrary::load(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/villages/world.json"),
    )
    .unwrap();
    let world = demo::world(&library).unwrap();
    let mut camera = Camera::new(Vec3::new(-286., -183., 25.));
    camera.set_distance(180.);
    camera.set_coverage(library.config.camera.coverage);
    let terrain: Vec<_> = library
        .config
        .items
        .iter()
        .enumerate()
        .filter(|(_, item)| item.name.starts_with("terrain-"))
        .map(|(i, _)| &world.items()[i])
        .collect();
    assert_eq!(terrain.len(), 400);
    let mut frame = Frame::default();
    for zoom in [0.65, 0.1, 0.025] {
        camera.set_zoom(zoom);
        for (w, h) in [(1280, 800), (800, 1280)] {
            camera.set_viewport(w, h);
            camera.orbit(0.5, 0.);
            frame
                .build(&world, &camera, 1, &library, 1., &[], false)
                .unwrap();
            let (right, up, _) = camera.basis();
            let (hw, hh) = camera.half_view();
            let mut visible = 0;
            for item in &terrain {
                let bounds = item.visibility_bounds();
                let delta = bounds.center() - camera.target();
                // Independent screen-plane expectation, without the radial cutoff under test.
                if delta.dot(right).abs() <= hw + bounds.projected_radius(right)
                    && delta.dot(up).abs() <= hh + bounds.projected_radius(up)
                {
                    visible += 1;
                    assert!(
                        frame.instances.iter().any(|i| i.item_id == item.id),
                        "missing tile {} at zoom {zoom}",
                        item.id
                    );
                }
            }
            if zoom == 0.025 {
                assert_eq!(visible, 400);
            }
            assert_eq!(camera.render_distance(), 180.);
        }
    }
}

#[test]
fn dialogue_households_and_offscreen_schedules_use_real_world_items() {
    use io_world::WorldView;
    let (mut game, mut world, ids) = fixture();
    let distant = ids["v2-n1"];
    let before = world.item(distant).unwrap().transform.anchor;
    let snapshot = game.clone();
    let frozen = world.snapshot();
    tick(&mut game, &mut world, 10.);
    assert_ne!(world.item(distant).unwrap().transform.anchor, before);
    assert_eq!(frozen.item(distant).unwrap().transform.anchor, before);
    assert_eq!(snapshot.village().unwrap().schedule_updates, 0);
    assert!(game.village().unwrap().schedule_updates > 40);
    assert_eq!(game.village().unwrap().npc_count(), 60);
    game.explore_command(
        &mut world,
        io_village::Command::Talk {
            target: Some(distant),
        },
    )
    .unwrap_err();
}

#[test]
fn recruit_follow_local_combat_assistance_and_return_to_exploration() {
    let (mut game, mut world, ids) = fixture();
    let hero = ids["hero"];
    let friend = ids["v0-companion"];
    game.explore_command(
        &mut world,
        io_village::Command::Talk {
            target: Some(friend),
        },
    )
    .unwrap();
    game.explore_command(&mut world, io_village::Command::Recruit)
        .unwrap();
    game.explore_command(&mut world, io_village::Command::Recruit)
        .unwrap();
    assert_eq!(game.village().unwrap().companion_count(), 1);
    let start = world.item(friend).unwrap().transform.anchor;
    game.command(
        &mut world,
        GameCommand::Move {
            actor: hero,
            window: io_game::MovementWindow::Exploration,
            x: 1.,
            y: 0.,
        },
    )
    .unwrap();
    tick(&mut game, &mut world, 2.);
    assert_ne!(world.item(friend).unwrap().transform.anchor, start);
    let d =
        world.item(friend).unwrap().transform.anchor - world.item(hero).unwrap().transform.anchor;
    assert!(d.dot(d) < 100.);
    let enemy = ids["v0-raider0"];
    let p = world.item(enemy).unwrap().transform.anchor;
    world.set_pose(hero, p - Vec3::new(7., 0., 0.), 0.);
    world.set_pose(friend, p - Vec3::new(5., 2., 0.), 0.);
    game.command(&mut world, GameCommand::StartCombat).unwrap();
    assert_eq!(game.actors().len(), 5);
    assert!(game.actor(ids["v1-raider0"]).is_none());
    assert!(matches!(game.phase(), Phase::Ready { round: 1 }));
    assert!(game.movement_window().is_none());
    let waiting = world.item(hero).unwrap().transform.anchor;
    tick(&mut game, &mut world, 4.);
    assert_eq!(world.item(hero).unwrap().transform.anchor, waiting);
    assert!(matches!(game.phase(), Phase::Ready { round: 1 }));
    game.command(&mut world, GameCommand::StartCombat).unwrap();
    tick(&mut game, &mut world, 3.);
    assert!(matches!(game.phase(), Phase::Turns { .. }));
    assert_eq!(
        game.command(
            &mut world,
            GameCommand::Move {
                actor: hero,
                window: io_game::MovementWindow::Exploration,
                x: 1.,
                y: 0.
            }
        ),
        Err(GameError::WrongPhase)
    );
    game.command(&mut world, GameCommand::EndTurn { actor: hero })
        .unwrap();
    tick(&mut game, &mut world, 1.);
    let Game::Village(session) = &game else {
        unreachable!()
    };
    assert!(
        session
            .events()
            .iter()
            .any(|e| matches!(e.payload,CombatEvent::Hit{source,..} if source==friend)),
        "companion must actually attack"
    );
    for k in 0..3 {
        world.damage(ids[&format!("v0-raider{k}")], 1000);
    }
    tick(&mut game, &mut world, 2.);
    assert!(game.village().unwrap().exploring());
    assert_eq!(game.village().unwrap().companion_count(), 1);
    assert!(!Encounter::alive(&world, enemy));
    assert!(world.item(enemy).unwrap().physics_body.is_some());
    assert!(Encounter::alive(&world, ids["v1-raider0"]));
    let p = world.item(ids["v1-raider0"]).unwrap().transform.anchor;
    world.set_pose(hero, p - Vec3::new(7., 0., 0.), 0.);
    game.command(&mut world, GameCommand::StartCombat).unwrap();
    assert!(matches!(game.phase(),Phase::Ready{round} if *round>1));
    game.command(&mut world, GameCommand::StartCombat).unwrap();
    assert!(matches!(game.phase(),Phase::Movement{round,..} if *round>1));
    assert_eq!(
        game.command(
            &mut world,
            GameCommand::Move {
                actor: hero,
                window: io_game::MovementWindow::Round(1),
                x: 1.,
                y: 0.
            }
        ),
        Err(GameError::WrongPhase)
    );
}

#[test]
fn visibility_gates_entry_and_lost_contact_returns_to_wandering() {
    let (mut game, mut world, ids) = fixture();
    let hero = ids["hero"];
    let enemy = ids["v0-raider0"];
    let home = world.item(ids["v0-house4"]).unwrap().transform.anchor;
    let left = home - Vec3::new(9., 0., 0.);
    let right = home + Vec3::new(9., 0., 0.);
    world.set_pose(hero, left, 0.);
    world.set_pose(enemy, right, 0.);
    assert!(!io_world::line_of_sight(&world, hero, enemy).unwrap());
    assert_eq!(
        game.command(&mut world, GameCommand::StartCombat),
        Err(GameError::OutOfRange)
    );
    tick(&mut game, &mut world, 1.);
    assert!(game.village().unwrap().exploring());
    world.set_pose(enemy, left + Vec3::new(0., 8., 0.), 0.);
    assert!(io_world::line_of_sight(&world, hero, enemy).unwrap());
    tick(&mut game, &mut world, 0.25);
    assert!(matches!(game.phase(), Phase::Ready { .. }));
    assert_eq!(
        game.actors().len(),
        2,
        "hidden/distant raiders must not join"
    );
    game.command(&mut world, GameCommand::StartCombat).unwrap();
    world.set_pose(enemy, right, 0.);
    // The building blocks pursuit and sight. Brief occlusion must not end combat.
    tick(&mut game, &mut world, 0.5);
    assert!(!game.village().unwrap().exploring());
    tick(&mut game, &mut world, 1.5);
    assert!(game.village().unwrap().exploring());
    tick(&mut game, &mut world, 1.);
    assert!(
        game.village().unwrap().exploring(),
        "must not immediately reenter through cover"
    );
    assert!(Encounter::alive(&world, enemy));
}

#[test]
fn grounded_exploration_cannot_walk_through_a_house() {
    let (mut game, mut world, ids) = fixture();
    let hero = ids["hero"];
    let home = world.item(ids["v0-house4"]).unwrap().transform.anchor;
    world.set_pose(hero, home - Vec3::new(0., 6., 0.), 0.);
    game.command(
        &mut world,
        GameCommand::Move {
            actor: hero,
            window: io_game::MovementWindow::Exploration,
            x: 0.,
            y: 1.,
        },
    )
    .unwrap();
    tick(&mut game, &mut world, 2.);
    let p = world.item(hero).unwrap().transform.anchor;
    assert!(p.y < home.y - 3.3, "{p:?}");
    assert!((p.z - world.terrain().unwrap().height(p.x, p.y).unwrap()).abs() < 1e-4);
}
