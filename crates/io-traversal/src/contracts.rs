//! Reusable movement facts and requests. No enemy, patrol or combat semantics.
use io_game::FrameworkError;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Framework(FrameworkError),
    InvalidWorld,
    InvalidInput,
}
impl From<FrameworkError> for Error {
    fn from(e: FrameworkError) -> Self {
        Self::Framework(e)
    }
}
use io_types::Vec3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NavigationTicket {
    pub actor: u64,
    pub revision: u64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NavigationGoal {
    Position(Vec3),
    Approach {
        position: Vec3,
        distance: f32,
        repath_seconds: f32,
    },
}
impl NavigationGoal {
    pub(crate) fn validate(self) -> Result<(), NavigationError> {
        match self {
            Self::Position(p) if p.finite() => Ok(()),
            Self::Approach {
                position,
                distance,
                repath_seconds,
            } if position.finite()
                && distance.is_finite()
                && (0.1..=20.).contains(&distance)
                && repath_seconds.is_finite()
                && (0.1..=5.).contains(&repath_seconds) =>
            {
                Ok(())
            }
            _ => Err(NavigationError::InvalidGoal),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum NavigationRequest {
    /// Temporary scheduling/execution hint, not permission to ignore physical limits.
    Prioritize {
        ticket: NavigationTicket,
        priority: NavigationPriority,
        seconds: f32,
    },
    Start {
        actor: u64,
        goal: NavigationGoal,
    },
    Replace {
        ticket: NavigationTicket,
        goal: NavigationGoal,
    },
    Cancel {
        ticket: NavigationTicket,
    },
    /// Re-evaluate the same objective without cancelling an in-flight plan or
    /// safe background execution. Repeated notices are coalesced.
    Reconsider {
        ticket: NavigationTicket,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum NavigationPriority {
    #[default]
    Routine,
    Elevated,
    Urgent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavigationError {
    UnknownActor,
    StaleTicket,
    InvalidGoal,
    RevisionExhausted,
    InvalidConfiguration,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavigationStatus {
    Idle,
    Planning,
    Following,
    Arrived,
    Unreachable,
    Blocked,
    Failed,
}
impl From<crate::navigation::Status> for NavigationStatus {
    fn from(status: crate::navigation::Status) -> Self {
        match status {
            crate::navigation::Status::Planning => Self::Planning,
            crate::navigation::Status::Following => Self::Following,
            crate::navigation::Status::Arrived => Self::Arrived,
            crate::navigation::Status::Unreachable => Self::Unreachable,
            crate::navigation::Status::Blocked => Self::Blocked,
            crate::navigation::Status::Failed => Self::Failed,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ActorFeedback<F> {
    /// Current accepted objective (may have changed since the last execution tick).
    pub ticket: NavigationTicket,
    /// Objective that produced the execution result below; never relabel old execution as new.
    pub execution_ticket: NavigationTicket,
    pub tick: u64,
    pub status: NavigationStatus,
    pub execution: F,
    pub known_cells: usize,
}

#[derive(Clone, Copy, Debug)]
pub enum MovementEvent<E> {
    Navigation {
        ticket: NavigationTicket,
        tick: u64,
        status: NavigationStatus,
    },
    Execution {
        actor: u64,
        tick: u64,
        event: E,
    },
}

/// Returned directly to the plugin each tick, never recovered from lossy event history.
#[derive(Clone, Debug)]
pub struct MovementFrame<F, E> {
    pub tick: u64,
    pub actors: Vec<ActorFeedback<F>>,
    pub events: Vec<MovementEvent<E>>,
}
