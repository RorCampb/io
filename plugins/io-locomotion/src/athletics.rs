//! Optional route actions. Physical execution is delegated to the shared Motor.
use crate::paths::{length, path, support};
pub use crate::paths::{Action, Profile, Settings};
use crate::{Error, MotionEvent, Motor, MotorFeedback};
use io_game::PluginWorld;
use io_traversal::navigation::{
    Agent, Cell, Connection, Domain, Endpoints, Failure, Location, Navigation, RouteProvider,
    Stats, Waypoint,
};
use io_traversal::{Execution, Progress, TraversalExecutor};
use io_types::Vec3;
use io_world::{character_support, surface_candidates, SupportProbe, SurfaceQuery, WorldView};
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Node {
    cell: Cell,
    height_mm: i32,
}
#[derive(Clone, Debug)]
pub struct Routes {
    domain: Domain,
    heuristic_speed: f32,
}
impl Routes {
    fn node(&self, p: Vec3) -> Option<Node> {
        Some(Node {
            cell: self.domain.cell(p)?,
            height_mm: (p.z * 1000.).round() as i32,
        })
    }
    fn samples(&self, w: &dyn WorldView, p: Profile, cell: Cell, z: f32) -> Vec<Location<Node>> {
        let [x, y] = self.domain.xy(cell);
        if self.domain.cell(Vec3::new(x, y, z)) != Some(cell) {
            return vec![];
        }
        let rise = (p.jump_speed * p.jump_speed / (2. * p.gravity)).min(3.);
        let Ok(query) = SurfaceQuery::new(
            x,
            y,
            z - p.max_drop.max(p.step_height) - 0.01,
            z + rise + 0.01,
        ) else {
            return vec![];
        };
        surface_candidates(w, query.excluding(p.actor))
            .into_iter()
            .filter_map(|s| {
                let hit =
                    character_support(w, Some(p.actor), s.position, p.body, SupportProbe::CONTACT)
                        .ok()??;
                let node = self.node(hit.anchor)?;
                Some(Location {
                    node,
                    position: hit.anchor,
                })
            })
            .take(2)
            .collect()
    }
}
impl RouteProvider for Routes {
    type Config = Domain;
    type Node = Node;
    type Action = Action;
    type Profile = Profile;
    fn new(domain: Domain) -> Result<Self, &'static str> {
        domain.validate()?;
        if domain.cell_size < 0.5 {
            return Err("athletics sampling must be at least 0.5m");
        }
        Ok(Self {
            domain,
            heuristic_speed: 12.,
        })
    }
    fn validate_profile(p: Profile) -> Result<(), &'static str> {
        p.body.validate().map_err(|_| "invalid body")?;
        Settings {
            step_height: p.step_height,
            jump_distance: p.jump_distance,
            max_drop: p.max_drop,
        }
        .validate()
        .map_err(|_| "invalid abilities")?;
        if !p.speed.is_finite()
            || p.speed <= 0.
            || !p.gravity.is_finite()
            || p.gravity <= 0.
            || !p.jump_speed.is_finite()
            || p.jump_speed <= 0.
        {
            return Err("invalid movement profile");
        }
        Ok(())
    }
    fn invalidate(&mut self) {}
    fn begin(
        &self,
        w: &dyn WorldView,
        p: Profile,
        start: Vec3,
        goal: Vec3,
        _: Option<u64>,
    ) -> Result<Endpoints<Node, Action>, Failure> {
        let a = self.node(start).ok_or(Failure::Unreachable)?;
        let b = self.node(goal).ok_or(Failure::Unreachable)?;
        let pick = |node: Node, at: Vec3| {
            self.samples(w, p, node.cell, at.z).into_iter().find(|s| {
                length(s.position - at) < self.domain.cell_size
                    && path(w, p, at, s.position, Action::Walk).is_some()
            })
        };
        let first = pick(a, start).ok_or(Failure::Unreachable)?;
        let last = pick(b, goal).ok_or(Failure::Unreachable)?;
        Ok(Endpoints {
            start: first,
            goal: last.node,
            first: Waypoint {
                position: first.position,
                action: Action::Walk,
            },
            last: Waypoint {
                position: goal,
                action: Action::Walk,
            },
        })
    }
    fn destination_available(
        &self,
        w: &dyn WorldView,
        p: Profile,
        _: Option<u64>,
        goal: Vec3,
    ) -> bool {
        self.node(goal).is_some() && support(w, p, goal)
    }
    fn neighbors(
        &mut self,
        w: &dyn WorldView,
        p: Profile,
        from: Location<Node>,
        stats: &mut Stats,
    ) -> Vec<Connection<Node, Action>> {
        self.heuristic_speed = p.speed;
        let mut result = vec![];
        let reach = (p.jump_distance / self.domain.cell_size).floor().min(8.) as i32;
        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            for n in 1..=reach.max(1) {
                let cell = Cell(from.node.cell.0 + dx * n, from.node.cell.1 + dy * n);
                for to in self.samples(w, p, cell, from.position.z) {
                    stats.clearance_queries += 1;
                    let actions: &[Action] = if n as f32 * self.domain.cell_size <= 1.01 {
                        &[Action::Walk, Action::Step, Action::Jump]
                    } else {
                        &[Action::Jump]
                    };
                    if let Some((action, route)) = actions
                        .iter()
                        .find_map(|a| path(w, p, from.position, to.position, *a).map(|r| (*a, r)))
                    {
                        let penalty = match action {
                            Action::Walk => 0,
                            Action::Step => 100,
                            Action::Jump => 1500,
                        };
                        result.push(Connection {
                            destination: to,
                            action,
                            cost: (route.duration() * 1000.).ceil() as u64 + penalty + 1,
                        });
                    }
                }
            }
        }
        result
    }
    fn heuristic(&self, from: Node, to: Node) -> u64 {
        let dx = (from.cell.0 - to.cell.0) as f64 * self.domain.cell_size as f64;
        let dy = (from.cell.1 - to.cell.1) as f64 * self.domain.cell_size as f64;
        (dx.hypot(dy) / self.heuristic_speed as f64 * 1000.).floor() as u64
    }
    fn live_clear(
        &self,
        w: &dyn WorldView,
        p: Profile,
        _: Option<u64>,
        a: Vec3,
        b: Vec3,
        action: Action,
    ) -> bool {
        path(w, p, a, b, action).is_some()
    }
    fn segment_clear(
        &self,
        w: &dyn WorldView,
        p: Profile,
        id: Option<u64>,
        a: Vec3,
        b: Vec3,
        action: Action,
    ) -> bool {
        self.live_clear(w, p, id, a, b, action)
    }
    fn offset_destination(
        &self,
        w: &dyn WorldView,
        p: Profile,
        _: Option<u64>,
        reference: Vec3,
        offset: [f32; 2],
    ) -> Option<Vec3> {
        let delta = Vec3::new(offset[0], offset[1], 0.);
        if !delta.finite() || length(delta) > 20. {
            return None;
        }
        let scenery = crate::surface::Scenery(w);
        let walk =
            io_world::walk_character(&scenery, Some(p.actor), reference, delta, p.body).ok()?;
        (walk.reached && self.destination_available(w, p, Some(p.actor), walk.position))
            .then_some(walk.position)
    }
    // Explicit completion keeps step/jump boundaries and takeoff coordinates intact.
}

#[derive(Clone, Debug)]
pub struct Executor {
    motor: Motor,
    settings: Settings,
    progress: Progress,
}
impl Executor {
    pub fn new(motor: Motor, settings: Settings, world: &dyn WorldView) -> Result<Self, String> {
        settings.validate()?;
        motor.validate(world).map_err(|e| format!("{e:?}"))?;
        Ok(Self {
            motor,
            settings,
            progress: Progress::Running,
        })
    }
}
impl TraversalExecutor for Executor {
    type Routes = Routes;
    type Feedback = MotorFeedback;
    type Event = MotionEvent;
    fn item(&self) -> u64 {
        self.motor.actor()
    }
    fn validate(&self, w: &dyn WorldView) -> Result<(), Error> {
        self.motor.validate(w)
    }
    fn profile(&self, w: &dyn WorldView) -> Result<Profile, Error> {
        self.motor.traversal_profile(w, self.settings)
    }
    fn feedback(&self, w: &dyn WorldView) -> Result<MotorFeedback, Error> {
        self.motor.feedback(w, Vec3::default(), Vec3::default())
    }
    fn cancel(&mut self) {
        self.motor.cancel();
        self.progress = Progress::Running;
    }
    fn begin<E>(&mut self, w: &mut PluginWorld<E>, step: Waypoint<Action>) -> Result<(), Error> {
        self.progress = self.motor.begin_traversal(w, self.settings, step)?;
        Ok(())
    }
    fn execute<E>(
        &mut self,
        w: &mut PluginWorld<E>,
        step: Option<Waypoint<Action>>,
        dt: f32,
        _: &mut Navigation<Routes>,
        _: &Agent<Routes>,
    ) -> Result<Execution<MotorFeedback, MotionEvent>, Error> {
        let start = w
            .item(self.item())
            .ok_or(Error::InvalidWorld)?
            .transform
            .anchor;
        let events = self.motor.update(w, dt)?;
        if step.is_some() && self.progress != Progress::Blocked {
            self.progress = self.motor.traversal_progress();
        }
        let end = w
            .item(self.item())
            .ok_or(Error::InvalidWorld)?
            .transform
            .anchor;
        let velocity = (end - start).scaled(1. / dt);
        Ok(Execution {
            feedback: self.motor.feedback(w, velocity, velocity)?,
            events,
            progress: self.progress,
        })
    }
}
