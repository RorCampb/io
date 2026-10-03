//! Optional upright-body executor implementing the public traversal contract.
//! This module consumes the same provider/executor API available to other developers.
pub use crate::motor::{ClipDefinition, Command, Locomotion, Motion, MotionEvent, Motor};
pub use crate::steering::{
    AccelerationSteering, Steering, SteeringInput, SteeringOutput, SteeringSettings,
};
use crate::surface::{Action, Agent, MovementProfile, Navigation, SurfaceRoutes, Waypoint};
use crate::trajectory::{
    CornerRefiner, TrajectoryRefiner, TrajectoryRequest, TrajectorySettings, TrajectoryStage,
};
use io_game::stage::Stage;
use io_game::PluginWorld;
use io_traversal::{Error, NavigationError, NavigationTicket};
use io_traversal::{Execution, Progress, TraversalExecutor};
use io_types::Vec3;
use io_world::WorldView;
pub type Movement = io_traversal::Movement<SurfaceExecutor>;
pub type MovementEvent = io_traversal::MovementEvent<MotionEvent>;
pub type ActorFeedback = io_traversal::ActorFeedback<MotorFeedback>;
fn prepare_step<E>(
    handoff: &mut crate::pipeline::StepHandoff<'_, '_, E>,
    motor: &Motor,
    navigation: &mut Navigation,
    agent: &Agent,
    start: Vec3,
    action: Action,
    dt: f32,
) -> bool {
    navigation.record_steering_query();
    let delta = motor.planar_delta(dt, action == Action::Crouch);
    let domain = navigation.configuration();
    delta.x.hypot(delta.y) <= 6.
        && domain.cell(start).is_some()
        && domain.cell(start + delta).is_some()
        && (action != Action::Crouch || agent.profile().crouch_height.is_some())
        && handoff.prepare(motor.actor(), start, delta, agent.profile().shape(action))
}
pub type MovementFrame = io_traversal::MovementFrame<MotorFeedback, MotionEvent>;
#[derive(Clone, Copy, Debug)]
pub struct MotorFeedback {
    pub actor: u64,
    pub position: Vec3,
    pub requested_velocity: Vec3,
    pub actual_velocity: Vec3,
    /// Runtime support state; the actor retains its CharacterBody while airborne.
    pub grounded: bool,
    pub crouched: bool,
    pub motion: Motion,
}

pub struct ActorBinding {
    pub motor: Motor,
    pub can_crouch: bool,
    pub familiar_points: Vec<Vec3>,
    pub steering: SteeringSettings,
}
#[derive(Clone, Debug)]
pub struct SurfaceExecutor {
    motor: Motor,
    can_crouch: bool,
    settings: SteeringSettings,
    steering: Option<std::sync::Arc<dyn Steering>>,
    refiner: Option<std::sync::Arc<dyn TrajectoryRefiner>>,
    trajectory: TrajectoryStage,
    previous: Option<MotorFeedback>,
    stalled: f32,
    priority: io_traversal::NavigationPriority,
}
impl From<ActorBinding> for io_traversal::ActorBinding<SurfaceExecutor> {
    fn from(b: ActorBinding) -> Self {
        Self {
            look_ahead: b.steering.look_ahead,
            familiar_points: b.familiar_points,
            executor: SurfaceExecutor {
                motor: b.motor,
                can_crouch: b.can_crouch,
                settings: b.steering,
                steering: None,
                refiner: None,
                trajectory: Default::default(),
                previous: None,
                stalled: 0.,
                priority: Default::default(),
            },
        }
    }
}
impl SurfaceExecutor {
    pub fn local_trajectory(&self) -> &[Vec3] {
        self.trajectory.points()
    }
    pub fn trajectory_stats(&self) -> crate::trajectory::TrajectoryStats {
        self.trajectory.stats
    }
}
pub trait TrajectoryControl {
    fn set_trajectory_refiner<R: TrajectoryRefiner + 'static>(
        &mut self,
        actor: u64,
        settings: TrajectorySettings,
        refiner: R,
        world: &dyn WorldView,
    ) -> Result<NavigationTicket, NavigationError>;
}
impl TrajectoryControl for io_traversal::Movement<SurfaceExecutor> {
    fn set_trajectory_refiner<R: TrajectoryRefiner + 'static>(
        &mut self,
        actor: u64,
        settings: TrajectorySettings,
        refiner: R,
        world: &dyn WorldView,
    ) -> Result<NavigationTicket, NavigationError> {
        settings
            .validate()
            .map_err(|_| NavigationError::InvalidConfiguration)?;
        let mut executor = self
            .executor(actor)
            .ok_or(NavigationError::UnknownActor)?
            .clone();
        executor.settings.trajectory = Some(settings);
        executor.refiner = Some(std::sync::Arc::new(refiner));
        let ticket = self
            .actor(actor)
            .ok_or(NavigationError::UnknownActor)?
            .ticket;
        self.replace_executor(ticket, executor, world)
    }
}
pub trait SteeringControl {
    fn set_steering<S: Steering + 'static>(
        &mut self,
        actor: u64,
        steering: S,
        world: &dyn WorldView,
    ) -> Result<NavigationTicket, NavigationError>;
}
impl SteeringControl for io_traversal::Movement<SurfaceExecutor> {
    fn set_steering<S: Steering + 'static>(
        &mut self,
        actor: u64,
        steering: S,
        world: &dyn WorldView,
    ) -> Result<NavigationTicket, NavigationError> {
        let mut executor = self
            .executor(actor)
            .ok_or(NavigationError::UnknownActor)?
            .clone();
        executor.steering = Some(std::sync::Arc::new(steering));
        let ticket = self
            .actor(actor)
            .ok_or(NavigationError::UnknownActor)?
            .ticket;
        self.replace_executor(ticket, executor, world)
    }
}
impl TraversalExecutor for SurfaceExecutor {
    type Routes = SurfaceRoutes;
    type Feedback = MotorFeedback;
    type Event = MotionEvent;
    fn item(&self) -> u64 {
        self.motor.actor()
    }
    fn validate(&self, world: &dyn WorldView) -> Result<(), Error> {
        self.settings.validate().map_err(|_| Error::InvalidInput)?;
        self.motor.validate(world)
    }
    fn profile(&self, world: &dyn WorldView) -> Result<MovementProfile, Error> {
        let shape = world
            .item(self.item())
            .and_then(|i| i.character_body)
            .ok_or(Error::InvalidWorld)?;
        let s = self.motor.settings();
        Ok(MovementProfile {
            radius: shape.radius,
            max_slope: shape.max_slope,
            standing_height: s.standing_height,
            crouch_height: self.can_crouch.then_some(s.crouch_height),
            walk_speed: s.walk_speed,
            crouch_speed: s.crouch_speed,
        })
    }
    fn feedback(&self, world: &dyn WorldView) -> Result<MotorFeedback, Error> {
        Ok(MotorFeedback {
            actor: self.item(),
            position: world
                .item(self.item())
                .ok_or(Error::InvalidWorld)?
                .transform
                .anchor,
            requested_velocity: Vec3::default(),
            actual_velocity: Vec3::default(),
            grounded: self.motor.grounded(),
            crouched: self.motor.crouched(),
            motion: self.motor.motion(),
        })
    }
    fn cancel(&mut self) {
        self.motor.cancel();
        self.trajectory.clear();
        self.stalled = 0.;
    }
    fn navigation_priority(&mut self, priority: io_traversal::NavigationPriority) {
        if priority > self.priority {
            self.trajectory.clear();
        }
        self.priority = priority;
    }
    fn execute<E>(
        &mut self,
        world: &mut PluginWorld<E>,
        next: Option<Waypoint>,
        dt: f32,
        navigation: &mut Navigation,
        agent: &Agent,
    ) -> Result<Execution<MotorFeedback, MotionEvent>, Error> {
        let actor = self.item();
        let start = world
            .item(actor)
            .ok_or(Error::InvalidWorld)?
            .transform
            .anchor;
        let default_steering =
            AccelerationSteering::new(self.settings).map_err(|_| Error::InvalidInput)?;
        let mut progress = Progress::Running;
        let mut requested_velocity = Vec3::default();
        let mut yielding = false;
        let mut complete_corner = false;
        let mut handoff = crate::pipeline::StepHandoff::new(world);
        self.motor.command(Command::Move { x: 0., y: 0. })?;
        if let Some(next) = next {
            let crouched = next.action == Action::Crouch;
            self.motor.command(Command::Crouch(crouched))?;
            let speed = if crouched {
                self.motor.settings().crouch_speed
            } else {
                self.motor.settings().walk_speed
            };
            let velocity = self.previous.map_or(Vec3::default(), |f| f.actual_velocity);
            let local = match self.settings.trajectory {
                Some(mut settings) => {
                    if self.priority != io_traversal::NavigationPriority::Routine {
                        settings.refresh_seconds = settings.refresh_seconds.min(0.05);
                    }
                    Some(self.trajectory.run(TrajectoryRequest {
                        world: handoff.view(),
                        navigation,
                        agent,
                        next,
                        position: start,
                        velocity,
                        speed,
                        braking: self.settings.braking,
                        seconds: dt,
                        settings,
                        refiner: self.refiner.as_deref().unwrap_or(&CornerRefiner),
                    })?)
                }
                None => None,
            };
            let target = local.map_or(next.position, |l| l.position);
            let curved_target = !self.trajectory.points().is_empty();
            let limit = local
                .map_or(speed, |l| l.speed)
                .max((velocity.x.hypot(velocity.y) - self.settings.braking * dt).max(0.1))
                .min(speed);
            complete_corner = local.is_some_and(|l| l.complete_corner);
            if local.is_some_and(|l| l.reconsider) {
                progress = Progress::Reconsider;
            }
            requested_velocity = self
                .steering
                .as_deref()
                .unwrap_or(&default_steering)
                .steer(SteeringInput {
                    actor,
                    position: start,
                    target,
                    current_velocity: velocity,
                    max_speed: limit,
                    seconds: dt,
                })?
                .velocity();
            if requested_velocity.x.hypot(requested_velocity.y) > speed + 0.00001 {
                return Err(Error::InvalidInput);
            }
            if local.is_some() {
                let change = requested_velocity - velocity;
                let budget = self.settings.braking.max(self.settings.acceleration) * dt;
                requested_velocity =
                    velocity + change.scaled((budget / change.x.hypot(change.y).max(1e-6)).min(1.));
            }
            if self.priority == io_traversal::NavigationPriority::Urgent {
                // Supplied locomotion policy: brake while the leased conflict is
                // imminent. Do not accelerate into a stale route or reset the ticket.
                let planar = Vec3::new(velocity.x, velocity.y, 0.);
                let speed = planar.x.hypot(planar.y);
                requested_velocity =
                    planar.scaled((1. - self.settings.braking * dt / speed.max(1e-6)).max(0.));
                yielding = true;
                complete_corner = false;
            }
            self.motor.command(Command::Move {
                x: requested_velocity.x / speed,
                y: requested_velocity.y / speed,
            })?;
            if !prepare_step(
                &mut handoff,
                &self.motor,
                navigation,
                agent,
                start,
                next.action,
                dt,
            ) {
                yielding = navigation.steering_segment_clear(
                    &crate::surface::Scenery(handoff.view()),
                    agent,
                    start,
                    start + requested_velocity.scaled(dt),
                    next.action,
                );
                // Safety outranks acceleration: stop, then turn from rest along the safe route.
                requested_velocity = Vec3::default();
                self.trajectory.clear();
                complete_corner = false;
                if !yielding && curved_target {
                    // A blocked rounded continuation must not pull us back toward
                    // the coarse corner we have already started bypassing.
                    progress = Progress::Blocked;
                } else if !yielding && self.priority != io_traversal::NavigationPriority::Urgent {
                    // A prepared route is not permission to cross changed scenery.
                    // Try turning from rest before reporting an invalid connector;
                    // inertia alone must not cause a needless route replacement.
                    let direct = self
                        .steering
                        .as_deref()
                        .unwrap_or(&default_steering)
                        .steer(SteeringInput {
                            actor,
                            position: start,
                            target: next.position,
                            current_velocity: Vec3::default(),
                            max_speed: speed,
                            seconds: dt,
                        })?
                        .velocity();
                    if direct.x.hypot(direct.y) > speed + 0.00001 {
                        return Err(Error::InvalidInput);
                    }
                    self.motor.command(Command::Move {
                        x: direct.x / speed,
                        y: direct.y / speed,
                    })?;
                    if prepare_step(
                        &mut handoff,
                        &self.motor,
                        navigation,
                        agent,
                        start,
                        next.action,
                        dt,
                    ) {
                        requested_velocity = direct;
                    } else {
                        progress = Progress::Blocked;
                    }
                }
            }
            let direction = requested_velocity.scaled(1. / speed);
            self.motor.command(Command::Move {
                x: direction.x,
                y: direction.y,
            })?;
        } else if agent.status() == io_traversal::navigation::Status::Arrived {
            self.motor.command(Command::Crouch(false))?;
        }
        let events = self.motor.update_prepared(handoff, dt)?;
        let end = world
            .item(actor)
            .ok_or(Error::InvalidWorld)?
            .transform
            .anchor;
        // Slow, deliberate convergence to a tight waypoint is progress, not a
        // blocked motor. Compare execution with this tick's requested travel.
        let requested_travel = requested_velocity.scaled(dt);
        let minimum_progress = (requested_travel.dot(requested_travel) * 0.01).max(1e-14);
        if yielding {
            self.stalled = 0.;
        } else if next.is_some() && (end - start).dot(end - start) < minimum_progress {
            self.stalled += dt;
            if self.stalled >= 0.75 {
                progress = Progress::Blocked;
                self.stalled = 0.;
            }
        } else if next.is_some() {
            self.stalled = 0.;
        }

        let feedback = MotorFeedback {
            actor,
            position: end,
            requested_velocity,
            actual_velocity: (end - start).scaled(1. / dt),
            grounded: self.motor.grounded(),
            crouched: self.motor.crouched(),
            motion: self.motor.motion(),
        };
        self.previous = Some(feedback);
        if complete_corner && matches!(progress, Progress::Running | Progress::Reconsider) {
            progress = Progress::Complete;
        }
        Ok(Execution {
            feedback,
            events,
            progress,
        })
    }
}
