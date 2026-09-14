use super::*;

#[test]
fn confirmed_rounds_are_frozen_until_explicit_begin() {
    let mut round = Round::<(), ()>::starting_at(0.25, 7).unwrap();
    round.set_confirmation(true).unwrap();
    assert_eq!(round.start(), Ok(7));
    assert_eq!(round.phase(), &RoundPhase::Ready { round: 7 });
    assert_eq!(round.movement_window(), None);
    assert_eq!(
        round.advance_movement(0.25, vec![1]),
        Err(RoundError::WrongPhase)
    );
    assert_eq!(round.set_confirmation(false), Err(RoundError::WrongPhase));
    assert_eq!(round.begin_movement(), Ok(7));
    assert_eq!(round.begin_movement(), Err(RoundError::WrongPhase));
    assert_eq!(round.movement_window(), Some(MovementWindow::Round(7)));
    round.advance_movement(0.25, vec![1]).unwrap();
    assert_eq!(round.end_turn(|_| true), Ok(TurnProgress::NewRound(8)));
    assert_eq!(round.phase(), &RoundPhase::Ready { round: 8 });
}

#[test]
fn round_owns_transitions_but_not_game_rules() {
    let mut round = Round::<&str, &str>::new(0.2).unwrap();
    assert_eq!(round.movement_window(), Some(MovementWindow::Exploration));
    assert_eq!(round.end_turn(|_| true), Err(RoundError::WrongPhase));
    assert_eq!(
        round.begin_resolution("crafting"),
        Err(RoundError::WrongPhase)
    );
    round.start().unwrap();
    assert_eq!(round.start(), Err(RoundError::WrongPhase));
    assert!(!round.advance_movement(0.1, vec![]).unwrap());
    assert!(round.advance_movement(0.1, vec![10, 20, 30]).unwrap());
    assert_eq!(round.active_actor(), Some(10));
    assert_eq!(round.movement_window(), None);
    round.begin_resolution("crafting").unwrap();
    assert_eq!(round.end_turn(|_| true), Err(RoundError::WrongPhase));
    *round.resolution_mut().unwrap() = "ready";
    round.complete_resolution().unwrap();
    assert_eq!(round.end_turn(|id| id != 20), Ok(TurnProgress::Actor(30)));
    assert_eq!(round.end_turn(|_| true), Ok(TurnProgress::NewRound(2)));
    round.finish(Some("recipe completed"));
    assert_eq!(
        round.phase(),
        &RoundPhase::Finished {
            outcome: Some("recipe completed")
        }
    );
    round.return_to_exploration().unwrap();
    round.start().unwrap();
    assert_eq!(round.movement_window(), Some(MovementWindow::Round(3)));
}

#[test]
fn invalid_duration_and_order_do_not_mutate_phase() {
    for dt in [f64::NAN, f64::INFINITY, 0., -1., 61.] {
        assert!(Round::<(), ()>::new(dt).is_err());
    }
    let mut round = Round::<(), ()>::new(0.1).unwrap();
    round.start().unwrap();
    let before = round.phase().clone();
    for order in [vec![], vec![0], vec![2, 2], vec![1; 4097]] {
        assert_eq!(
            round.advance_movement(0.1, order),
            Err(RoundError::InvalidOrder)
        );
        assert_eq!(round.phase(), &before);
    }
    for dt in [f64::NAN, f64::INFINITY, 0., -1., 0.3] {
        assert_eq!(
            round.advance_movement(dt, vec![1]),
            Err(RoundError::InvalidDuration)
        );
        assert_eq!(round.phase(), &before);
    }
    assert_eq!(round.return_to_exploration(), Err(RoundError::WrongPhase));
}
