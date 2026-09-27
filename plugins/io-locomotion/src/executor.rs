//! Optional upright-body executor implementing the public traversal contract.
//! This module consumes the same provider/executor API available to other developers.
pub use crate::motor::{ClipDefinition, Command, Locomotion, Motion, MotionEvent, Motor};
pub use crate::steering::{
    AccelerationSteering, Steering, SteeringInput, SteeringOutput, SteeringSettings,
};
use crate::surface::{Action, Agent, MovementProfile, Navigation, SurfaceRoutes, Waypoint};
use io_game::PluginWorld;
use io_traversal::{Error, NavigationError, NavigationTicket};
use io_traversal::{Execution, Progress, TraversalExecutor};
use io_types::Vec3;
use io_world::WorldView;
pub type Movement = io_traversal::Movement<SurfaceExecutor>;
pub type MovementEvent = io_traversal::MovementEvent<MotionEvent>;
pub type ActorFeedback = io_traversal::ActorFeedback<MotorFeedback>;
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
    previous: Option<MotorFeedback>,
    stalled: f32,
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
                previous: None,
                stalled: 0.,
            },
        }
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
        self.stalled = 0.;
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
        self.motor.command(Command::Move { x: 0., y: 0. })?;
        if let Some(next) = next {
            let crouched = next.action == Action::Crouch;
            self.motor.command(Command::Crouch(crouched))?;
            let speed = if crouched {
                self.motor.settings().crouch_speed
            } else {
                self.motor.settings().walk_speed
            };
            requested_velocity = self
                .steering
                .as_deref()
                .unwrap_or(&default_steering)
                .steer(SteeringInput {
                    actor,
                    position: start,
                    target: next.position,
                    current_velocity: self.previous.map_or(Vec3::default(), |f| f.actual_velocity),
                    max_speed: speed,
                    seconds: dt,
                })?
                .velocity();
            if requested_velocity.x.hypot(requested_velocity.y) > speed + 0.00001 {
                return Err(Error::InvalidInput);
            }
            if !navigation.steering_segment_clear(
                world,
                agent,
                start,
                start + requested_velocity.scaled(dt),
                next.action,
            ) {
                yielding = navigation.steering_segment_clear(
                    &crate::surface::Scenery(world),
                    agent,
                    start,
                    start + requested_velocity.scaled(dt),
                    next.action,
                );
                // Safety outranks acceleration: stop, then turn from rest along the safe route.
                requested_velocity = Vec3::default();
                if !yielding {
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
                    if navigation.steering_segment_clear(
                        world,
                        agent,
                        start,
                        start + direct.scaled(dt),
                        next.action,
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
        let events = self.motor.update(world, dt)?;
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
        Ok(Execution {
            feedback,
            events,
            progress,
        })
    }
}
