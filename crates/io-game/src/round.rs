//! Phase ownership independent of factions, abilities, models and input devices.
use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq)]
pub enum RoundPhase<R, O> {
    Exploration,
    Ready {
        round: u32,
    },
    Movement {
        round: u32,
        remaining: f64,
    },
    Turns {
        round: u32,
        order: Vec<u64>,
        cursor: usize,
    },
    Resolving {
        round: u32,
        order: Vec<u64>,
        cursor: usize,
        resolution: R,
    },
    Finished {
        outcome: Option<O>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MovementWindow {
    Exploration,
    Round(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoundError {
    WrongPhase,
    InvalidDuration,
    InvalidOrder,
    Exhausted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TurnProgress {
    Actor(u64),
    NewRound(u32),
}

/// Plugins supply a validated participant order and resolution payload, not phase writes.
#[derive(Clone, Debug)]
pub struct Round<R, O> {
    phase: RoundPhase<R, O>,
    movement_seconds: f64,
    last_round: u32,
    confirmation: bool,
}

impl<R, O> Round<R, O> {
    pub fn starting_at(movement_seconds: f64, first_round: u32) -> Result<Self, RoundError> {
        let mut round = Self::new(movement_seconds)?;
        round.last_round = first_round.checked_sub(1).ok_or(RoundError::Exhausted)?;
        Ok(round)
    }
    pub fn new(movement_seconds: f64) -> Result<Self, RoundError> {
        if !movement_seconds.is_finite() || !(0.1..=60.).contains(&movement_seconds) {
            return Err(RoundError::InvalidDuration);
        }
        Ok(Self {
            phase: RoundPhase::Exploration,
            movement_seconds,
            last_round: 0,
            confirmation: false,
        })
    }
    pub fn phase(&self) -> &RoundPhase<R, O> {
        &self.phase
    }
    pub fn set_confirmation(&mut self, enabled: bool) -> Result<(), RoundError> {
        if !matches!(self.phase, RoundPhase::Exploration) {
            return Err(RoundError::WrongPhase);
        }
        self.confirmation = enabled;
        Ok(())
    }
    pub fn begin_movement(&mut self) -> Result<u32, RoundError> {
        let RoundPhase::Ready { round } = self.phase else {
            return Err(RoundError::WrongPhase);
        };
        self.phase = RoundPhase::Movement {
            round,
            remaining: self.movement_seconds,
        };
        Ok(round)
    }
    fn next_phase(&self, round: u32) -> RoundPhase<R, O> {
        if self.confirmation {
            RoundPhase::Ready { round }
        } else {
            RoundPhase::Movement {
                round,
                remaining: self.movement_seconds,
            }
        }
    }
    pub fn active_actor(&self) -> Option<u64> {
        match &self.phase {
            RoundPhase::Turns { order, cursor, .. }
            | RoundPhase::Resolving { order, cursor, .. } => order.get(*cursor).copied(),
            RoundPhase::Exploration
            | RoundPhase::Ready { .. }
            | RoundPhase::Movement { .. }
            | RoundPhase::Finished { .. } => None,
        }
    }
    pub fn movement_window(&self) -> Option<MovementWindow> {
        match self.phase {
            RoundPhase::Exploration => Some(MovementWindow::Exploration),
            RoundPhase::Movement { round, .. } => Some(MovementWindow::Round(round)),
            RoundPhase::Ready { .. }
            | RoundPhase::Turns { .. }
            | RoundPhase::Resolving { .. }
            | RoundPhase::Finished { .. } => None,
        }
    }
    pub fn start(&mut self) -> Result<u32, RoundError> {
        if !matches!(self.phase, RoundPhase::Exploration) {
            return Err(RoundError::WrongPhase);
        }
        let round = self
            .last_round
            .checked_add(1)
            .ok_or(RoundError::Exhausted)?;
        self.last_round = round;
        self.phase = self.next_phase(round);
        Ok(round)
    }
    /// Order is supplied when movement expires, allowing the plugin to filter live participants.
    pub fn advance_movement(&mut self, dt: f64, order: Vec<u64>) -> Result<bool, RoundError> {
        if !dt.is_finite() || dt <= 0. || dt > 0.25 {
            return Err(RoundError::InvalidDuration);
        }
        let RoundPhase::Movement { round, remaining } = self.phase else {
            return Err(RoundError::WrongPhase);
        };
        if remaining - dt > 1e-7 {
            self.phase = RoundPhase::Movement {
                round,
                remaining: remaining - dt,
            };
            return Ok(false);
        }
        let unique: BTreeSet<_> = order.iter().copied().collect();
        if order.is_empty()
            || order.len() > 4096
            || unique.len() != order.len()
            || unique.contains(&0)
        {
            return Err(RoundError::InvalidOrder);
        }
        self.phase = RoundPhase::Turns {
            round,
            order,
            cursor: 0,
        };
        Ok(true)
    }
    pub fn begin_resolution(&mut self, resolution: R) -> Result<(), RoundError> {
        let RoundPhase::Turns {
            round,
            order,
            cursor,
        } = &self.phase
        else {
            return Err(RoundError::WrongPhase);
        };
        self.phase = RoundPhase::Resolving {
            round: *round,
            order: order.clone(),
            cursor: *cursor,
            resolution,
        };
        Ok(())
    }
    pub fn resolution_mut(&mut self) -> Option<&mut R> {
        match &mut self.phase {
            RoundPhase::Resolving { resolution, .. } => Some(resolution),
            _ => None,
        }
    }
    pub fn complete_resolution(&mut self) -> Result<(), RoundError> {
        let RoundPhase::Resolving {
            round,
            order,
            cursor,
            ..
        } = &self.phase
        else {
            return Err(RoundError::WrongPhase);
        };
        self.phase = RoundPhase::Turns {
            round: *round,
            order: order.clone(),
            cursor: *cursor,
        };
        Ok(())
    }
    /// Eligibility is a plugin rule (alive, connected, stunned, etc.), not an engine rule.
    pub fn end_turn(&mut self, eligible: impl Fn(u64) -> bool) -> Result<TurnProgress, RoundError> {
        let RoundPhase::Turns {
            round,
            order,
            cursor,
        } = &self.phase
        else {
            return Err(RoundError::WrongPhase);
        };
        let mut next = *cursor + 1;
        while next < order.len() && !eligible(order[next]) {
            next += 1;
        }
        if next == order.len() {
            let round = round.checked_add(1).ok_or(RoundError::Exhausted)?;
            self.last_round = round;
            self.phase = self.next_phase(round);
            Ok(TurnProgress::NewRound(round))
        } else {
            let actor = order[next];
            self.phase = RoundPhase::Turns {
                round: *round,
                order: order.clone(),
                cursor: next,
            };
            Ok(TurnProgress::Actor(actor))
        }
    }
    pub fn finish(&mut self, outcome: Option<O>) {
        self.phase = RoundPhase::Finished { outcome };
    }
    /// Return to exploration on the same world; round IDs never restart within this instance.
    pub fn return_to_exploration(&mut self) -> Result<(), RoundError> {
        if !matches!(self.phase, RoundPhase::Finished { .. }) {
            return Err(RoundError::WrongPhase);
        }
        self.phase = RoundPhase::Exploration;
        Ok(())
    }
}

#[cfg(test)]
mod exhaustion_tests {
    use super::*;
    #[test]
    fn exhausted_round_ids_cannot_wrap_or_partially_transition() {
        let mut round = Round::<(), ()>::new(0.1).unwrap();
        round.last_round = u32::MAX;
        assert_eq!(round.start(), Err(RoundError::Exhausted));
        assert_eq!(round.phase(), &RoundPhase::Exploration);
        round.phase = RoundPhase::Turns {
            round: u32::MAX,
            order: vec![1],
            cursor: 0,
        };
        let before = round.phase().clone();
        assert_eq!(round.end_turn(|_| true), Err(RoundError::Exhausted));
        assert_eq!(round.phase(), &before);
    }
}
