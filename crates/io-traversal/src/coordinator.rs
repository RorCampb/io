//! Per-Item objective, ticket and action lifecycle. Physical movement belongs to D.
use crate::approach::Approach;
use crate::navigation::{
    Agent, HandoffRequest, Navigation, RouteHandoff, RouteProvider, Status, Waypoint,
};
use crate::planning::{PlanJob, PlanKey, Planner, PlanningMode};
use crate::*;
use io_game::{stage::Stage, PluginWorld};
use io_types::Vec3;
use io_world::WorldView;
pub(crate) struct PlanningTick<'a, P: RouteProvider> {
    pub budget: usize,
    pub service: &'a mut Planner<P>,
}

#[derive(Clone, Debug)]
pub(crate) struct RouteCoordinator<D: TraversalExecutor> {
    pub executor: D,
    follower: RouteFollower<D::Routes>,
    familiar_points: Vec<Vec3>,
    goal: Option<NavigationGoal>,
    pending: bool,
    approach: Approach,
    current_goal: Vec3,
    goal_age: f32,
    retries: u8,
    retry_delay: f32,
    reported: (u64, NavigationStatus),
    pub feedback: ActorFeedback<D::Feedback>,
    active_step: Option<Waypoint<<D::Routes as RouteProvider>::Action>>,
    inputs_revision: Option<u64>,
    background: bool,
    attempt: u64,
    inflight: Option<PlanKey>,
    executing_ticket: NavigationTicket,
    plan_start: Vec3,
    reconsider_after_plan: bool,
    inline_reconsider: Option<(Agent<D::Routes>, Vec3)>,
    reconsider_requested: bool,
}
impl<D: TraversalExecutor> RouteCoordinator<D> {
    pub fn set_planning(&mut self, mode: PlanningMode) {
        self.background = mode == PlanningMode::Background;
        self.inflight = None;
        self.inline_reconsider = None;
        self.reconsider_requested = false;
        self.reconsider_after_plan = false;
        self.pending = self.goal.is_some();
        self.executor.cancel();
        self.active_step = None;
        self.follower.agent.retry();
    }
    pub fn route(&self) -> Option<&[Waypoint<<D::Routes as RouteProvider>::Action>]> {
        if (self.pending && !self.background) || self.goal.is_none() {
            return None;
        }
        self.follower.agent.remaining_route()
    }
    pub fn new(binding: ActorBinding<D>, world: &dyn WorldView) -> Result<Self, String> {
        binding
            .executor
            .validate(world)
            .map_err(|e| format!("executor: {e:?}"))?;
        if binding.familiar_points.len() > 256
            || binding.familiar_points.iter().any(|p| !p.finite())
        {
            return Err("invalid familiar points".into());
        }
        let actor = binding.executor.item();
        let item = world.item(actor).ok_or("unknown traversal item")?;
        let profile = binding
            .executor
            .profile(world)
            .map_err(|e| format!("profile: {e:?}"))?;
        let agent = Agent::for_actor(actor, profile, item.transform.anchor)?;
        let feedback = ActorFeedback {
            ticket: NavigationTicket { actor, revision: 0 },
            execution_ticket: NavigationTicket { actor, revision: 0 },
            tick: 0,
            status: NavigationStatus::Idle,
            known_cells: 0,
            execution: binding
                .executor
                .feedback(world)
                .map_err(|e| format!("feedback: {e:?}"))?,
        };
        Ok(Self {
            background: false,
            attempt: 0,
            inflight: None,
            executing_ticket: feedback.ticket,
            plan_start: item.transform.anchor,
            reconsider_after_plan: false,
            inline_reconsider: None,
            reconsider_requested: false,
            active_step: None,
            inputs_revision: None,
            executor: binding.executor,
            follower: RouteFollower::new(agent, binding.look_ahead)?,
            familiar_points: binding.familiar_points,
            goal: None,
            pending: false,
            approach: Default::default(),
            current_goal: item.transform.anchor,
            goal_age: 0.,
            retries: 0,
            retry_delay: 0.,
            reported: (0, NavigationStatus::Idle),
            feedback,
        })
    }
    pub fn request(
        &mut self,
        expected: Option<NavigationTicket>,
        goal: Option<NavigationGoal>,
    ) -> Result<NavigationTicket, NavigationError> {
        if expected.is_some_and(|ticket| ticket != self.feedback.ticket) {
            return Err(NavigationError::StaleTicket);
        }
        if let Some(goal) = goal {
            goal.validate()?;
        }
        let revision = self
            .feedback
            .ticket
            .revision
            .checked_add(1)
            .ok_or(NavigationError::RevisionExhausted)?;
        self.goal = goal;
        self.reconsider_after_plan = false;
        self.inline_reconsider = None;
        self.reconsider_requested = false;
        self.inflight = None;
        self.pending = goal.is_some();
        self.approach.clear();
        self.goal_age = 0.;
        self.retries = 0;
        self.retry_delay = 0.;
        self.feedback.ticket.revision = revision;
        self.feedback.status = if goal.is_some() {
            NavigationStatus::Planning
        } else {
            NavigationStatus::Idle
        };
        if !self.background || goal.is_none() || self.follower.status() != Status::Following {
            self.executor.cancel();
            self.active_step = None;
        }
        if goal.is_none() {
            self.follower.agent.retry();
        }
        Ok(self.feedback.ticket)
    }
    pub fn reconsider(
        &mut self,
        ticket: NavigationTicket,
    ) -> Result<NavigationTicket, NavigationError> {
        if ticket != self.feedback.ticket {
            return Err(NavigationError::StaleTicket);
        }
        if self.goal.is_some() {
            if self.background {
                self.reconsider_after_plan |= self.inflight.is_some();
                self.pending = true;
            } else {
                self.reconsider_requested = true;
            }
            self.retry_delay = 0.;
        }
        Ok(ticket)
    }
    pub fn replace_executor(
        &mut self,
        ticket: NavigationTicket,
        mut executor: D,
        world: &dyn WorldView,
    ) -> Result<NavigationTicket, NavigationError> {
        if ticket != self.feedback.ticket {
            return Err(NavigationError::StaleTicket);
        }
        if executor.item() != ticket.actor {
            return Err(NavigationError::InvalidConfiguration);
        }
        executor
            .validate(world)
            .map_err(|_| NavigationError::InvalidConfiguration)?;
        executor
            .feedback(world)
            .map_err(|_| NavigationError::InvalidConfiguration)?;
        let profile = executor
            .profile(world)
            .map_err(|_| NavigationError::InvalidConfiguration)?;
        <D::Routes as RouteProvider>::validate_profile(profile)
            .map_err(|_| NavigationError::InvalidConfiguration)?;
        // Validate before changing the accepted objective or cancelling the old executor.
        let next = self.request(Some(ticket), self.goal)?;
        executor.cancel();
        self.executor = executor;
        self.inputs_revision = None;
        Ok(next)
    }
    fn set_goal(&mut self, goal: Vec3) -> Result<(), Error> {
        self.follower
            .set_goal(goal)
            .map_err(|_| Error::InvalidInput)?;
        self.executor.cancel();
        self.active_step = None;
        self.current_goal = goal;
        self.retries = 0;
        self.retry_delay = 0.;
        Ok(())
    }
    pub fn prepare(
        &mut self,
        nav: &Navigation<D::Routes>,
        world: &dyn WorldView,
    ) -> Result<(), Error> {
        if self.executor.item() != self.feedback.ticket.actor {
            return Err(Error::InvalidWorld);
        }
        let profile = self.executor.profile(world)?;
        let revision = nav.revision(world);
        if self.inputs_revision.is_none()
            || (!nav.live_checked() && self.inputs_revision != Some(revision))
            || profile != self.follower.agent.profile()
        {
            self.executor.cancel();
            self.active_step = None;
            self.follower
                .agent
                .set_profile(profile)
                .map_err(|_| Error::InvalidInput)?;
            self.follower.agent.retry();
            self.inputs_revision = Some(revision);
            self.inflight = None;
            self.pending = self.goal.is_some();
            self.inline_reconsider = None;
        }
        Ok(())
    }
    fn background_target<E>(
        &mut self,
        nav: &mut Navigation<D::Routes>,
        world: &mut PluginWorld<E>,
        start: Vec3,
        dt: f32,
        planning: PlanningTick<'_, D::Routes>,
    ) -> Result<Option<Waypoint<<D::Routes as RouteProvider>::Action>>, Error> {
        let planner = planning.service;
        let actor = self.executor.item();
        let Some(goal) = self.goal else {
            planner.cancel(actor);
            self.inflight = None;
            self.executing_ticket = self.feedback.ticket;
            return Ok(None);
        };
        if let Some(key) = self.inflight {
            if let Some(mut result) = planner.take(key) {
                self.inflight = None;
                nav.add_stats(result.stats);
                if result.key.ticket == self.feedback.ticket
                    && RouteHandoff
                        .run(HandoffRequest {
                            navigation: nav,
                            world,
                            agent: &mut result.agent,
                            planned_start: result.start,
                            position: start,
                            horizon: self.follower.horizon(),
                            snapshot_revision: result.revision,
                            current_profile: self.follower.agent.profile(),
                        })
                        .map_err(|_| Error::InvalidWorld)?
                    && (self.follower.status() != Status::Following
                        || matches!(result.agent.status(), Status::Following | Status::Arrived))
                {
                    self.executor.cancel();
                    self.active_step = None;
                    self.follower.agent = result.agent;
                    self.executing_ticket = result.key.ticket;
                    self.pending = self.reconsider_after_plan;
                    self.reconsider_after_plan = false;
                    self.familiar_points.clear();
                    self.goal_age = 0.;
                    planner.accepted();
                } else {
                    self.pending = true;
                    planner.rejected();
                }
            } else if !planner.contains(key) {
                // A cloned simulation owns no original runtime/queue.
                self.inflight = None;
                self.pending = true;
            }
        }
        if !self.pending {
            let moved = start - self.plan_start;
            let retry = match self.follower.status() {
                Status::Blocked => true,
                Status::Unreachable => {
                    matches!(goal, NavigationGoal::Approach { .. }) || moved.dot(moved) > 0.0001
                }
                Status::Planning => true,
                _ => false,
            };
            if retry {
                self.retry_delay -= dt;
                if self.retry_delay <= 0. {
                    self.pending = true;
                    self.retries = self.retries.saturating_add(1);
                    self.retry_delay = (1u32 << self.retries.min(3)) as f32;
                }
            }
        }
        if self.pending
            && self.inflight.is_none()
            && planning.budget > 0
            && planner.admission_available(actor)
        {
            self.attempt = self.attempt.checked_add(1).ok_or(Error::InvalidInput)?;
            let key = PlanKey {
                ticket: self.feedback.ticket,
                attempt: self.attempt,
            };
            let job = PlanJob {
                key,
                revision: nav.revision(world),
                start,
                goal,
                agent: self.follower.agent.clone(),
                world: std::sync::Arc::new(world.snapshot()),
                familiar: self.familiar_points.clone(),
                horizon: self.follower.horizon(),
            };
            if planner.submit(job, nav.configuration())? {
                self.inflight = Some(key);
                self.plan_start = start;
            }
        }
        // Zero search budget only follows an existing route, with live safety checks.
        Ok(self.follower.next_target(nav, world, start, 0))
    }
    pub fn update<E>(
        &mut self,
        nav: &mut Navigation<D::Routes>,
        world: &mut PluginWorld<E>,
        dt: f32,
        tick: u64,
        planning: PlanningTick<'_, D::Routes>,
        events: &mut Vec<MovementEvent<D::Event>>,
    ) -> Result<(), Error> {
        let actor = self.executor.item();
        self.prepare(nav, world)?;
        let start = world
            .item(actor)
            .ok_or(Error::InvalidWorld)?
            .transform
            .anchor;
        self.goal_age += dt;
        let (mut budget, planner) = (planning.budget, planning.service);
        if !self.background {
            if self.reconsider_requested && self.inline_reconsider.is_none() {
                self.reconsider_requested = false;
                if self.follower.status() == Status::Following {
                    let mut agent = self.follower.agent.clone();
                    agent.retry();
                    self.inline_reconsider = Some((agent, start));
                } else if self.follower.status() != Status::Planning {
                    self.pending = self.goal.is_some();
                }
            }
            if let Some((agent, planned_start)) = &mut self.inline_reconsider {
                nav.advance(world, agent, *planned_start, budget);
                budget = 0;
                if agent.status() != Status::Planning {
                    if matches!(agent.status(), Status::Following | Status::Arrived) {
                        nav.refine_route(world, agent, *planned_start, self.follower.horizon())
                            .map_err(|_| Error::InvalidWorld)?;
                        if RouteHandoff
                            .run(HandoffRequest {
                                snapshot_revision: nav.revision(world),
                                navigation: nav,
                                world,
                                agent,
                                planned_start: *planned_start,
                                position: start,
                                horizon: self.follower.horizon(),
                                current_profile: self.follower.agent.profile(),
                            })
                            .map_err(|_| Error::InvalidWorld)?
                        {
                            self.follower.agent = agent.clone();
                            self.executor.cancel();
                            self.active_step = None;
                        } else {
                            self.reconsider_requested = true;
                        }
                    }
                    self.inline_reconsider = None;
                }
            }
        }
        let (next, enabled) = if self.background {
            let next = self.background_target(
                nav,
                world,
                start,
                dt,
                PlanningTick {
                    budget,
                    service: planner,
                },
            )?;
            (next, self.goal.is_some())
        } else {
            match self.goal {
                Some(NavigationGoal::Position(goal)) if self.pending => self.set_goal(goal)?,
                Some(NavigationGoal::Approach {
                    position,
                    distance,
                    repath_seconds,
                }) => {
                    if self.pending
                        || (self.goal_age >= repath_seconds
                            && (!self.approach.has_destination()
                                || self.follower.status() != Status::Planning))
                    {
                        self.goal_age = 0.;
                        if let Some(goal) = self.approach.destination(
                            nav,
                            world,
                            &self.follower.agent,
                            start,
                            position,
                            distance,
                        ) {
                            if self.pending || goal != self.current_goal {
                                self.set_goal(goal)?;
                            }
                        }
                    }
                }
                _ => {}
            }
            self.pending = false;
            if budget > 0 {
                if let Some(point) = self.familiar_points.pop() {
                    nav.familiarize(world, &mut self.follower.agent, &[point]);
                }
            }
            let enabled = match self.goal {
                None => false,
                Some(NavigationGoal::Position(_)) => true,
                Some(NavigationGoal::Approach { .. }) => self.approach.has_destination(),
            };
            if enabled && self.follower.status() == Status::Blocked {
                self.retry_delay -= dt;
                if self.retry_delay <= 0. {
                    self.follower.agent.retry();
                    self.retries = self.retries.saturating_add(1);
                    self.retry_delay = (1u32 << self.retries.min(3)) as f32;
                }
            }
            let next = if enabled {
                let horizon = self.follower.horizon();
                nav.advance(world, &mut self.follower.agent, start, budget);
                nav.refine_route(world, &mut self.follower.agent, start, horizon)
                    .map_err(|_| Error::InvalidWorld)?;
                nav.prepared_target(&mut self.follower.agent, start, horizon)
            } else {
                None
            };
            self.executing_ticket = self.feedback.ticket;
            (next, enabled)
        };

        if next != self.active_step {
            self.executor.cancel();
            if let Some(step) = next {
                if let Err(error) = self.executor.begin(world, step) {
                    self.executor.cancel();
                    self.follower.agent.fail();
                    self.active_step = None;
                    return Err(error);
                }
            }
            self.active_step = next;
        }
        let execution = match self
            .executor
            .execute(world, next, dt, nav, &self.follower.agent)
        {
            Ok(result) => result,
            Err(error) => {
                self.executor.cancel();
                self.follower.agent.fail();
                self.active_step = None;
                return Err(error);
            }
        };
        match execution.progress {
            Progress::Running => {}
            Progress::Complete if next.is_some() => {
                self.follower.agent.complete_step();
                self.executor.cancel();
                self.active_step = None;
            }
            Progress::Complete => {}
            Progress::Blocked => {
                self.follower.agent.block();
                self.executor.cancel();
                self.active_step = None;
            }
            Progress::Failed => {
                self.follower.agent.fail();
                self.executor.cancel();
                self.active_step = None;
            }
        }
        for event in execution.events {
            events.push(MovementEvent::Execution { actor, tick, event });
        }
        let status = match self.goal {
            None => NavigationStatus::Idle,
            Some(_)
                if self.background
                    && self.pending
                    && (self.follower.status() != Status::Following
                        || self.executing_ticket != self.feedback.ticket) =>
            {
                NavigationStatus::Planning
            }
            Some(_) if !enabled => NavigationStatus::Unreachable,
            Some(_) => self.follower.status().into(),
        };
        let reported = (self.feedback.ticket.revision, status);
        if reported != self.reported {
            events.push(MovementEvent::Navigation {
                ticket: self.feedback.ticket,
                tick,
                status,
            });
            if status == NavigationStatus::Blocked {
                self.retry_delay = (1u32 << self.retries.min(3)) as f32;
            }
            self.reported = reported;
        }
        self.feedback = ActorFeedback {
            ticket: self.feedback.ticket,
            execution_ticket: self.executing_ticket,
            tick,
            status,
            known_cells: self.follower.agent.knowledge().known_cells(),
            execution: execution.feedback,
        };
        Ok(())
    }
}
