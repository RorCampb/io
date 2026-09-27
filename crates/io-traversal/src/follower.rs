use crate::navigation::{Agent, Navigation, RouteProvider, Status, Waypoint};
use io_types::Vec3;
use io_world::WorldView;

/// Owns progress along a route; does not move characters or choose gameplay objectives.
#[derive(Clone, Debug)]
pub struct RouteFollower<P: RouteProvider> {
    pub(crate) agent: Agent<P>,
    look_ahead: f32,
}
impl<P: RouteProvider> RouteFollower<P> {
    pub fn new(agent: Agent<P>, look_ahead: f32) -> Result<Self, &'static str> {
        if !look_ahead.is_finite() || look_ahead <= 0. {
            return Err("invalid route look-ahead distance");
        }
        Ok(Self { agent, look_ahead })
    }
    pub fn status(&self) -> Status {
        self.agent.status()
    }
    pub(crate) fn horizon(&self) -> f32 {
        self.look_ahead
    }
    pub fn set_goal(&mut self, goal: Vec3) -> Result<(), &'static str> {
        self.agent.set_goal(goal)
    }
    pub fn next_target(
        &mut self,
        nav: &mut Navigation<P>,
        world: &dyn WorldView,
        position: Vec3,
        budget: usize,
    ) -> Option<Waypoint<P::Action>> {
        nav.advance(world, &mut self.agent, position, budget);
        nav.prepared_target(&mut self.agent, position, self.look_ahead)
    }
}
