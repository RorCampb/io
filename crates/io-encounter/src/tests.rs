use super::*;
use io_world::{Durability, Item, Space, World};

fn fixture() -> (Game, World) {
    let mut items: Vec<_> = (1..=3)
        .map(|id| Item {
            id,
            durability: Some(Durability::new(30, 30).unwrap()),
            ..Item::default()
        })
        .collect();
    items[1].transform.anchor.x = 1.;
    items[2].transform.anchor.x = 8.;
    let world = World::new(Space::new(Vec3::new(100., 100., 100.)), items);
    let definition: GameDefinition = serde_json::from_str(r#"{
        "version":1,"movement_seconds":0.25,
        "abilities":{"sword":{"range":2,"effect":{"type":"damage","amount":5}},
                     "spell":{"range":20,"effect":{"type":"damage","amount":15}}},
        "templates":{"hero":{"control":"player","faction":1,"initiative":20,"movement_speed":20,"abilities":["sword","spell"],"opportunity_ability":"sword"},
                     "enemy":{"control":"player","faction":2,"initiative":10,"movement_speed":20,"abilities":["sword"],"opportunity_ability":"sword"}},
        "combatants":[{"item":"hero","template":"hero"},{"item":"enemy","template":"enemy"},{"item":"other","template":"enemy"}]
    }"#).unwrap();
    let names = BTreeMap::from([("hero".into(), 1), ("enemy".into(), 2), ("other".into(), 3)]);
    (register(definition, &world, &names).unwrap(), world)
}
fn begin_turns(game: &mut Game, world: &mut World) {
    game.command(world, GameCommand::StartCombat).unwrap();
    game.step(world, &[], 0.25).unwrap();
    assert_eq!(game.active_actor(), Some(1));
}

#[test]
fn plugin_events_are_typed_ordered_and_rejected_actions_emit_nothing() {
    let (mut game, mut world) = fixture();
    begin_turns(&mut game, &mut world);
    assert_eq!(game.events()[0].payload, CombatEvent::RoundStarted(1));
    let frozen = game.clone();
    let ability = game.actor(1).unwrap().abilities()[0];
    game.command(
        &mut world,
        GameCommand::Attack {
            actor: 1,
            target: 2,
            ability,
        },
    )
    .unwrap();
    assert_eq!(
        game.events()[1].payload,
        CombatEvent::Hit {
            source: 1,
            target: 2,
            damage: 5,
            opportunity: false
        }
    );
    assert_eq!(
        game.command(&mut world, GameCommand::EndTurn { actor: 1 }),
        Err(GameError::NotYourTurn)
    );
    assert_eq!(game.events().len(), 2);
    game.command(&mut world, GameCommand::EndTurn { actor: 2 })
        .unwrap();
    game.command(&mut world, GameCommand::EndTurn { actor: 3 })
        .unwrap();
    assert_eq!(
        game.events().back().unwrap().payload,
        CombatEvent::RoundStarted(2)
    );
    assert_eq!(frozen.events().len(), 1);
    for (i, event) in game.events().iter().enumerate() {
        assert_eq!(event.sequence, i as u64 + 1);
    }
}

#[test]
fn rounds_repeat_movement_ordered_turns_and_health_stays_in_world() {
    let (mut game, mut world) = fixture();
    begin_turns(&mut game, &mut world);
    let phase = game.phase().clone();
    let items = world.items().to_vec();
    assert_eq!(
        game.command(&mut world, GameCommand::EndTurn { actor: 2 }),
        Err(GameError::NotYourTurn)
    );
    assert_eq!(
        game.command(
            &mut world,
            GameCommand::Move {
                actor: 1,
                window: MovementWindow::Round(1),
                x: 1.,
                y: 0.
            }
        ),
        Err(GameError::WrongPhase)
    );
    assert_eq!(game.phase(), &phase);
    assert_eq!(world.items(), items);
    let sword = game.actor(1).unwrap().abilities()[0];
    game.command(
        &mut world,
        GameCommand::Attack {
            actor: 1,
            target: 2,
            ability: sword,
        },
    )
    .unwrap();
    assert_eq!(
        world
            .item(2)
            .unwrap()
            .durability
            .as_ref()
            .unwrap()
            .current(),
        25
    );
    assert_eq!(game.active_actor(), Some(2));
    game.command(&mut world, GameCommand::EndTurn { actor: 2 })
        .unwrap();
    game.command(&mut world, GameCommand::EndTurn { actor: 3 })
        .unwrap();
    assert!(matches!(game.phase(), Phase::Movement { round: 2, .. }));
}

#[test]
fn rejected_attacks_and_invalid_input_are_atomic() {
    let (mut game, mut world) = fixture();
    begin_turns(&mut game, &mut world);
    let sword = game.actor(1).unwrap().abilities()[0];
    for (command, error) in [
        (
            GameCommand::Attack {
                actor: 1,
                target: 3,
                ability: sword,
            },
            GameError::OutOfRange,
        ),
        (
            GameCommand::Attack {
                actor: 1,
                target: 1,
                ability: sword,
            },
            GameError::FriendlyTarget,
        ),
        (
            GameCommand::Attack {
                actor: 1,
                target: 2,
                ability: AbilityId(999),
            },
            GameError::UnknownAbility,
        ),
        (
            GameCommand::Move {
                actor: 1,
                window: MovementWindow::Round(1),
                x: f32::NAN,
                y: 0.,
            },
            GameError::InvalidValue,
        ),
    ] {
        let phase = game.phase().clone();
        let items = world.items().to_vec();
        assert_eq!(game.command(&mut world, command), Err(error));
        assert_eq!(game.phase(), &phase);
        assert_eq!(world.items(), items);
    }
    assert_eq!(
        game.step(&mut world, &[], f32::NAN),
        Err(GameError::InvalidValue)
    );
}

#[test]
fn opportunity_attack_crossing_spends_once_and_resets_next_round() {
    let (mut game, mut world) = fixture();
    game.command(&mut world, GameCommand::StartCombat).unwrap();
    assert!(game.engagements().contains(&(2, 1)));
    game.command(
        &mut world,
        GameCommand::Move {
            actor: 1,
            window: MovementWindow::Round(1),
            x: -1.,
            y: 0.,
        },
    )
    .unwrap();
    game.step(&mut world, &[], 0.2).unwrap();
    assert_eq!(
        world
            .item(1)
            .unwrap()
            .durability
            .as_ref()
            .unwrap()
            .current(),
        25
    );
    assert!(!game.actor(2).unwrap().reaction_available());
    assert!(!game.engagements().contains(&(2, 1)));
    game.step(&mut world, &[], 0.05).unwrap();
    for actor in [1, 2, 3] {
        game.command(&mut world, GameCommand::EndTurn { actor })
            .unwrap();
    }
    assert!(game.actor(2).unwrap().reaction_available());
    assert!(matches!(game.phase(), Phase::Movement { round: 2, .. }));
}

#[test]
fn lethal_reaction_stops_at_boundary_and_dead_actors_lose_turns() {
    let (mut game, mut world) = fixture();
    world.damage(1, 26);
    game.command(&mut world, GameCommand::StartCombat).unwrap();
    game.command(
        &mut world,
        GameCommand::Move {
            actor: 1,
            window: MovementWindow::Round(1),
            x: -1.,
            y: 0.,
        },
    )
    .unwrap();
    game.step(&mut world, &[], 0.25).unwrap();
    assert!(!Encounter::alive(&world, 1));
    assert!((world.item(1).unwrap().transform.anchor.x + 1.).abs() < 1e-5);
    assert!(matches!(game.phase(), Phase::Finished { outcome: Some(2) }));
}

#[test]
fn movement_timer_clamps_final_tick_and_diagonal_speed() {
    let (mut game, mut world) = fixture();
    game.command(&mut world, GameCommand::StartCombat).unwrap();
    game.command(
        &mut world,
        GameCommand::Move {
            actor: 1,
            window: MovementWindow::Round(1),
            x: 1.,
            y: 1.,
        },
    )
    .unwrap();
    game.step(&mut world, &[], 0.2).unwrap();
    game.step(&mut world, &[], 0.2).unwrap();
    let p = world.item(1).unwrap().transform.anchor;
    assert!((p.dot(p).sqrt() - 5.).abs() < 1e-5);
    game.step(&mut world, &[], 0.2).unwrap();
    assert_eq!(world.item(1).unwrap().transform.anchor, p);
}

#[test]
fn definitions_validate_after_editing_and_bind_exclusive_movement() {
    let (game, world) = fixture();
    let mut definition = (**game.definition.as_ref().unwrap()).clone();
    definition.templates.get_mut("hero").unwrap().movement_speed = f32::NAN;
    assert!(definition.validate().is_err());
    let mut json = serde_json::to_value(game.definition.as_ref().unwrap().as_ref()).unwrap();
    json["abilities"]["sword"]["effect"]["unexpected"] = true.into();
    assert!(serde_json::from_value::<GameDefinition>(json).is_err());
    let names = BTreeMap::from([("hero".into(), 1), ("enemy".into(), 2), ("other".into(), 3)]);
    let mut items = world.items().to_vec();
    items[0].durability = None;
    let invalid = World::new(world.space().clone(), items);
    assert!(register(
        (**game.definition.as_ref().unwrap()).clone(),
        &invalid,
        &names
    )
    .is_err());
}

#[test]
fn snapshots_are_independent_and_command_envelope_is_compact() {
    fn transferable<T: Send + Sync + Copy + 'static>() {}
    transferable::<GameCommand>();
    assert!(std::mem::size_of::<io_types::Envelope<GameCommand>>() <= 64);
    let (mut game, mut world) = fixture();
    let snapshot = game.clone();
    game.command(&mut world, GameCommand::StartCombat).unwrap();
    assert_eq!(snapshot.phase(), &Phase::Exploration);
}

#[test]
fn swept_threat_crossings_do_not_require_an_inside_endpoint() {
    let exit = exit_fraction(Vec3::new(-3., 0., 0.), Vec3::new(3., 0., 0.), 2.).unwrap();
    assert!((exit - 5. / 6.).abs() < 1e-6);
    assert!(exit_fraction(Vec3::new(-3., 3., 0.), Vec3::new(3., 3., 0.), 2.).is_none());
}

#[test]
fn npc_turn_is_delayed_and_resolves_without_player_commands() {
    let (game, mut world) = fixture();
    let mut def = (**game.definition.as_ref().unwrap()).clone();
    def.templates.get_mut("enemy").unwrap().control = Control::Npc;
    let names = BTreeMap::from([("hero".into(), 1), ("enemy".into(), 2), ("other".into(), 3)]);
    let mut game = register(def, &world, &names).unwrap();
    begin_turns(&mut game, &mut world);
    game.command(&mut world, GameCommand::EndTurn { actor: 1 })
        .unwrap();
    assert_eq!(
        game.command(&mut world, GameCommand::EndTurn { actor: 2 }),
        Err(GameError::NotPlayer)
    );
    game.step(&mut world, &[], 0.25).unwrap();
    assert_eq!(game.active_actor(), Some(2));
    game.step(&mut world, &[], 0.25).unwrap();
    assert_eq!(game.active_actor(), Some(3));
    assert_eq!(
        world
            .item(1)
            .unwrap()
            .durability
            .as_ref()
            .unwrap()
            .current(),
        25
    );
}

#[test]
fn death_transfers_to_physics_once_and_settles_offscreen() {
    use io_world::{BodyKind, Collider, ColliderShape, PhysicsBody};
    let (game, world) = fixture();
    let mut definition = (**game.definition.as_ref().unwrap()).clone();
    definition.templates.get_mut("enemy").unwrap().death_physics = Some(DeathPhysics {
        mass: 2.,
        knockback_impulse: 4.,
        lift_impulse: 3.,
        half_extents: [0.5; 3],
        offset: [0.5; 3],
    });
    definition.abilities.get_mut("spell").unwrap().effect = AbilityEffect::Damage { amount: 100 };
    let mut items = world.items().to_vec();
    let mut floor = Item {
        id: 4,
        physics_body: Some(PhysicsBody::new(BodyKind::Static)),
        ..Item::default()
    };
    floor.transform.anchor = Vec3::new(-50., -50., -1.);
    let mut collider = Collider::new(ColliderShape::Box {
        half_extents: Vec3::new(50., 50., 0.5),
    });
    collider.offset = Vec3::new(50., 50., 0.5);
    floor.collider = Some(collider);
    items.push(floor);
    let mut world = World::new(world.space().clone(), items);
    let names = BTreeMap::from([("hero".into(), 1), ("enemy".into(), 2), ("other".into(), 3)]);
    let mut game = register(definition, &world, &names).unwrap();
    begin_turns(&mut game, &mut world);
    let spell = game.actor(1).unwrap().abilities()[1];
    game.command(
        &mut world,
        GameCommand::Attack {
            actor: 1,
            target: 2,
            ability: spell,
        },
    )
    .unwrap();
    let body = world
        .item(2)
        .unwrap()
        .physics_body
        .as_ref()
        .unwrap()
        .clone();
    assert_eq!(body.kind, BodyKind::Dynamic);
    assert!(body.velocity.x > 0. && body.velocity.z > 0.);
    assert!(body.angular_velocity.dot(body.angular_velocity) > 0.);
    assert_eq!(game.active_actor(), Some(3));
    assert_eq!(world.physics_indices().len(), 2);
    game.command(
        &mut world,
        GameCommand::Move {
            actor: 1,
            window: MovementWindow::Round(1),
            x: 0.,
            y: 0.,
        },
    )
    .unwrap();
    assert_eq!(world.item(2).unwrap().physics_body.as_ref().unwrap(), &body);
    let anchor = world.item(2).unwrap().transform.anchor;
    for _ in 0..600 {
        game.step(&mut world, &[], 1. / 60.).unwrap();
    }
    assert!(world.physics_error().is_none());
    let corpse = world.item(2).unwrap();
    assert!((corpse.transform.anchor.x - anchor.x).abs() > 0.05);
    assert!(corpse.physics_body.as_ref().unwrap().is_sleeping());
    assert!(corpse.transform.anchor.z > -1. && corpse.transform.anchor.z < 2.);
    assert_eq!(corpse.durability.as_ref().unwrap().current(), 0);
}

#[test]
fn body_attachment_rejects_invalid_input_without_mutating_world() {
    use io_world::{BodyKind, Collider, ColliderShape, PhysicsBody};
    let (_, mut world) = fixture();
    let before = world.items().to_vec();
    let body = PhysicsBody::new(BodyKind::Dynamic);
    let collider = Collider::new(ColliderShape::Sphere { radius: 0.5 });
    let revision = world.revision();
    assert!(world
        .attach_dynamic_body(
            1,
            body,
            collider,
            Vec3::new(f32::NAN, 0., 0.),
            Vec3::default()
        )
        .is_err());
    assert_eq!(world.items(), before);
    assert_eq!(world.revision(), revision);
    assert!(world.physics_indices().is_empty());
}

#[test]
fn held_input_stops_at_deadline_and_old_window_cannot_move_next_round() {
    let (mut game, mut world) = fixture();
    game.command(&mut world, GameCommand::StartCombat).unwrap();
    let held = GameCommand::Move {
        actor: 1,
        window: MovementWindow::Round(1),
        x: 0.,
        y: 1.,
    };
    game.command(&mut world, held).unwrap();
    game.step(&mut world, &[], 0.25).unwrap();
    let stopped = world.item(1).unwrap().transform.anchor;
    for _ in 0..100 {
        assert_eq!(game.command(&mut world, held), Err(GameError::WrongPhase));
        game.step(&mut world, &[], 1. / 144.).unwrap();
        assert_eq!(world.item(1).unwrap().transform.anchor, stopped);
    }
    for actor in [1, 2, 3] {
        game.command(&mut world, GameCommand::EndTurn { actor })
            .unwrap();
    }
    assert_eq!(game.command(&mut world, held), Err(GameError::WrongPhase));
    game.step(&mut world, &[], 0.1).unwrap();
    assert_eq!(world.item(1).unwrap().transform.anchor, stopped);
}

#[test]
fn projectile_tracks_selected_target_and_damage_occurs_only_on_impact() {
    let (game, mut world) = fixture();
    let mut def = (**game.definition.as_ref().unwrap()).clone();
    def.abilities.get_mut("spell").unwrap().delivery = AbilityDelivery::Projectile { speed: 8. };
    let names = BTreeMap::from([("hero".into(), 1), ("enemy".into(), 2), ("other".into(), 3)]);
    let mut game = register(def, &world, &names).unwrap();
    begin_turns(&mut game, &mut world);
    let ability = game.actor(1).unwrap().abilities()[1];
    game.command(
        &mut world,
        GameCommand::Attack {
            actor: 1,
            target: 3,
            ability,
        },
    )
    .unwrap();
    assert!(
        matches!(game.phase(),Phase::Resolving{resolution: projectile,..} if projectile.target==3)
    );
    assert_eq!(
        world
            .item(3)
            .unwrap()
            .durability
            .as_ref()
            .unwrap()
            .current(),
        30
    );
    assert!(game.damage_numbers().is_empty());
    assert_eq!(
        game.command(&mut world, GameCommand::EndTurn { actor: 1 }),
        Err(GameError::WrongPhase)
    );
    game.step(&mut world, &[], 0.25).unwrap();
    assert!(
        matches!(game.phase(),Phase::Resolving{resolution: projectile,..} if projectile.position.x>0.)
    );
    assert!(world.set_pose(3, Vec3::new(8., 2., 0.), 0.));
    for _ in 0..10 {
        if matches!(game.phase(), Phase::Resolving { .. }) {
            game.step(&mut world, &[], 0.25).unwrap();
        }
    }
    assert_eq!(
        world
            .item(2)
            .unwrap()
            .durability
            .as_ref()
            .unwrap()
            .current(),
        30
    );
    assert_eq!(
        world
            .item(3)
            .unwrap()
            .durability
            .as_ref()
            .unwrap()
            .current(),
        15
    );
    assert_eq!(game.damage_numbers().len(), 1);
    let number = &game.damage_numbers()[0];
    assert_eq!(number.target, 3);
    assert_eq!(number.amount, 15);
    assert_eq!(game.active_actor(), Some(2));
    for _ in 0..7 {
        game.step(&mut world, &[], 0.25).unwrap();
    }
    assert!(game.damage_numbers().is_empty());
}

#[test]
fn projectile_delivery_is_strict_and_cannot_be_an_opportunity_attack() {
    let (game, _) = fixture();
    let mut def = (**game.definition.as_ref().unwrap()).clone();
    def.abilities.get_mut("sword").unwrap().delivery = AbilityDelivery::Projectile { speed: 8. };
    assert!(def.validate().is_err());
    assert!(serde_json::from_str::<AbilityDelivery>(r#"{"type":"instant","speed":4}"#).is_err());
    def.abilities.get_mut("sword").unwrap().delivery = AbilityDelivery::default();
    def.abilities.get_mut("spell").unwrap().delivery =
        AbilityDelivery::Projectile { speed: f32::NAN };
    assert!(def.validate().is_err());
}
