//! Item-bound execution supplied by a developer. No humanoid or animation requirements.
use crate::navigation::{Agent, Navigation, RouteProvider, Waypoint};
use crate::Error;
use io_game::PluginWorld;
use io_world::WorldView;
use std::fmt::Debug;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Progress {
    Running,
    /// Ask for a replacement while retaining the currently executable route.
    Reconsider,
    Complete,
    Blocked,
    Failed,
}
#[derive(Clone, Debug)]
pub struct Execution<F, E> {
    pub feedback: F,
    pub events: Vec<E>,
    pub progress: Progress,
}

/// One implementation owns one Item's traversal execution. Mutable temporal state
/// must clone independently. Cancellation clears held intent synchronously; apply
/// any physical stop in before_step, before the host integrates physics.
pub trait TraversalExecutor: Clone + Debug + Send + Sync + 'static {
    type Routes: RouteProvider;
    type Feedback: Copy + Debug + Send + Sync;
    type Event: Copy + Debug + Send + Sync;
    fn item(&self) -> u64;
    fn validate(&self, world: &dyn WorldView) -> Result<(), Error>;
    fn profile(
        &self,
        world: &dyn WorldView,
    ) -> Result<<Self::Routes as RouteProvider>::Profile, Error>;
    fn feedback(&self, world: &dyn WorldView) -> Result<Self::Feedback, Error>;
    fn cancel(&mut self);
    /// Optional plugin response to a leased navigation hint. The engine assigns
    /// no movement semantics; Routine clears an expired or replaced hint.
    fn navigation_priority(&mut self, _priority: crate::NavigationPriority) {}
    /// Called for a newly selected target/action, before execute. Continuous
    /// look-ahead targets can change frequently; only hold action-specific state here.
    fn begin<E>(
        &mut self,
        _world: &mut PluginWorld<E>,
        _step: Waypoint<<Self::Routes as RouteProvider>::Action>,
    ) -> Result<(), Error> {
        Ok(())
    }
    fn before_step<E>(&mut self, _world: &mut PluginWorld<E>, _seconds: f32) -> Result<(), Error> {
        Ok(())
    }
    /// Called after world integration. None means no action is available: do not
    /// continue stale route intent, but independent physical state may still settle.
    fn execute<E>(
        &mut self,
        world: &mut PluginWorld<E>,
        step: Option<Waypoint<<Self::Routes as RouteProvider>::Action>>,
        seconds: f32,
        navigation: &mut Navigation<Self::Routes>,
        agent: &Agent<Self::Routes>,
    ) -> Result<Execution<Self::Feedback, Self::Event>, Error>;
}

#[derive(Clone, Debug)]
pub struct ActorBinding<D: TraversalExecutor> {
    pub executor: D,
    pub familiar_points: Vec<io_types::Vec3>,
    pub look_ahead: f32,
}
