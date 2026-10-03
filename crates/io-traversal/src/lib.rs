#![forbid(unsafe_code)]
//! Reusable movement execution. Game plugins own objectives, observations and reactions.
mod approach;
mod contracts;
mod coordinator;
mod executor;
mod follower;
pub mod navigation;
mod planning;
use crate::navigation::{Navigation, RouteProvider};
pub use contracts::*;
pub use executor::*;
pub use follower::RouteFollower;
use io_game::PluginWorld;
use io_types::Envelope;
use io_world::WorldView;
pub use planning::{PlanningMode, PlanningStats};

#[derive(Clone, Debug)]
pub struct Movement<D: TraversalExecutor> {
    navigation: Navigation<D::Routes>,
    actors: Vec<coordinator::RouteCoordinator<D>>,
    budget: usize,
    admission: planning::AdmissionOrder,
    tick: u64,
    planner: planning::Planner<D::Routes>,
}
impl<D: TraversalExecutor> Movement<D> {
    pub fn new<B: Into<ActorBinding<D>>>(
        domain: <D::Routes as RouteProvider>::Config,
        budget: usize,
        bindings: Vec<B>,
        world: &dyn WorldView,
    ) -> Result<Self, String> {
        if !(1..=128).contains(&budget) {
            return Err("invalid movement budget".into());
        }
        let mut ids = std::collections::HashSet::new();
        let mut actors = Vec::new();
        for binding in bindings {
            let binding = binding.into();
            if !ids.insert(binding.executor.item()) {
                return Err("duplicate movement authority".into());
            }
            actors.push(coordinator::RouteCoordinator::new(binding, world)?);
        }
        Ok(Self {
            navigation: Navigation::new(domain)?,
            actors,
            budget,
            admission: Default::default(),
            tick: 0,
            planner: planning::Planner::new(PlanningMode::Inline),
        })
    }
    /// Select scheduling before attaching to a session. Clone publications never share workers.
    pub fn with_planning(mut self, mode: PlanningMode) -> Self {
        self.planner = planning::Planner::new(mode);
        for actor in &mut self.actors {
            actor.set_planning(mode);
        }
        self
    }
    pub fn planning_stats(&self) -> PlanningStats {
        self.planner.stats()
    }
    /// Direct checked request/reply. Envelope IDs correlate calls; tickets identify objective revisions.
    /// Rejected requests preserve the current objective. Arrival is a later result, not acceptance.
    pub fn request(
        &mut self,
        request: Envelope<NavigationRequest>,
    ) -> Envelope<Result<NavigationTicket, NavigationError>> {
        let (actor, expected, goal) = match request.payload {
            NavigationRequest::Prioritize {
                ticket,
                priority,
                seconds,
            } => {
                let result = self
                    .actors
                    .iter_mut()
                    .find(|c| c.executor.item() == ticket.actor)
                    .ok_or(NavigationError::UnknownActor)
                    .and_then(|c| c.prioritize(ticket, priority, seconds));
                return request.reply(result);
            }
            NavigationRequest::Start { actor, goal } => (actor, None, Some(goal)),
            NavigationRequest::Replace { ticket, goal } => (ticket.actor, Some(ticket), Some(goal)),
            NavigationRequest::Cancel { ticket } => (ticket.actor, Some(ticket), None),
            NavigationRequest::Reconsider { ticket } => {
                let result = self
                    .actors
                    .iter_mut()
                    .find(|c| c.executor.item() == ticket.actor)
                    .ok_or(NavigationError::UnknownActor)
                    .and_then(|c| c.reconsider(ticket));
                return request.reply(result);
            }
        };
        let result = self
            .actors
            .iter_mut()
            .find(|c| c.executor.item() == actor)
            .ok_or(NavigationError::UnknownActor)
            .and_then(|c| c.request(expected, goal));
        if result.is_ok() {
            self.planner.cancel(actor);
        }
        request.reply(result)
    }
    /// Read-only access for plugin configuration/diagnostics; use replace_executor for changes.
    pub fn executor(&self, actor: u64) -> Option<&D> {
        self.actors
            .iter()
            .find(|c| c.feedback.ticket.actor == actor)
            .map(|c| &c.executor)
    }
    pub fn actor(&self, actor: u64) -> Option<ActorFeedback<D::Feedback>> {
        self.actors
            .iter()
            .find(|c| c.executor.item() == actor)
            .map(|c| c.feedback)
    }
    /// Read-only remaining actions for diagnostics, never a second route owner.
    pub fn route(
        &self,
        actor: u64,
    ) -> Option<&[navigation::Waypoint<<D::Routes as RouteProvider>::Action>]> {
        self.actors
            .iter()
            .find(|c| c.executor.item() == actor)?
            .route()
    }
    /// Accept a validated replacement for one Item's execution/configuration.
    /// Its objective is retained and replanned under a new ticket revision.
    pub fn replace_executor(
        &mut self,
        ticket: NavigationTicket,
        executor: D,
        world: &dyn WorldView,
    ) -> Result<NavigationTicket, NavigationError> {
        let next = self
            .actors
            .iter_mut()
            .find(|c| c.feedback.ticket.actor == ticket.actor)
            .ok_or(NavigationError::UnknownActor)?
            .replace_executor(ticket, executor, world)?;
        self.planner.cancel(ticket.actor);
        Ok(next)
    }
    pub fn before_step<E>(&mut self, world: &mut PluginWorld<E>, dt: f32) -> Result<(), Error> {
        if !dt.is_finite() || dt <= 0. || dt > 0.25 {
            return Err(Error::InvalidInput);
        }
        self.validate(world)?;
        for actor in &mut self.actors {
            actor.prepare(&self.navigation, world)?;
            actor.executor.before_step(world, dt)?;
        }
        Ok(())
    }
    pub fn actors(&self) -> impl Iterator<Item = ActorFeedback<D::Feedback>> + '_ {
        self.actors.iter().map(|c| c.feedback)
    }
    pub fn active_items(&self) -> Vec<u64> {
        self.actors.iter().map(|c| c.executor.item()).collect()
    }
    pub fn stats(&self) -> crate::navigation::Stats {
        self.navigation.stats()
    }
    pub fn validate(&self, world: &dyn WorldView) -> Result<(), Error> {
        for actor in &self.actors {
            if actor.executor.item() != actor.feedback.ticket.actor {
                return Err(Error::InvalidWorld);
            }
            actor.executor.validate(world)?;
        }
        Ok(())
    }
    /// One owner ticks this once per simulation step. Feedback is delivered directly, not queued.
    pub fn update<E>(
        &mut self,
        world: &mut PluginWorld<E>,
        dt: f32,
    ) -> Result<MovementFrame<D::Feedback, D::Event>, Error> {
        if !dt.is_finite() || dt <= 0. || dt > 0.25 {
            return Err(Error::InvalidInput);
        }
        self.validate(world)?;
        self.tick = self.tick.checked_add(1).ok_or(Error::InvalidInput)?;
        self.planner.poll();
        for actor in &mut self.actors {
            actor.advance_priority(dt);
            self.planner
                .prioritize(actor.feedback.ticket.actor, actor.priority);
        }
        // Three priority slices, then one unconditional round-robin slice.
        // Tie order rotates independently, so a constant urgent source cannot starve peers.
        let selected = self
            .admission
            .select(self.actors.len(), self.tick, |i| self.actors[i].priority);
        let mut events = Vec::new();
        for (i, actor) in self.actors.iter_mut().enumerate() {
            let budget = if i == selected { self.budget } else { 0 };
            actor.update(
                &mut self.navigation,
                world,
                dt,
                self.tick,
                coordinator::PlanningTick {
                    budget,
                    service: &mut self.planner,
                },
                &mut events,
            )?;
        }
        Ok(MovementFrame {
            tick: self.tick,
            actors: self.actors().collect(),
            events,
        })
    }
}
