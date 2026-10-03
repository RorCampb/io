//! Optional humanoid surface provider. Not required by the route search.
use crate::pipeline::{SceneryPath, SupportStage, WalkRequest};
use io_game::stage::Stage;
use io_traversal::navigation::*;
use io_types::Vec3;
use io_world::WorldView;
fn distance(a: Vec3, b: Vec3) -> f32 {
    let d = a - b;
    d.dot(d).sqrt()
}
use io_world::{
    character_supported_segment, walk_character, CharacterBody, Item, PhysicsStats, Space,
    SurfaceSource,
};
use serde::Deserialize;
use std::{collections::HashMap, sync::Arc};
pub type Agent = io_traversal::navigation::Agent<SurfaceRoutes>;
pub type Navigation = io_traversal::navigation::Navigation<SurfaceRoutes>;
pub type Waypoint = io_traversal::navigation::Waypoint<Action>;
/// Numeric capabilities, not a separate enum variant for every body size.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MovementProfile {
    pub radius: f32,
    #[serde(default = "default_max_slope")]
    pub max_slope: f32,
    pub standing_height: f32,
    pub crouch_height: Option<f32>,
    pub walk_speed: f32,
    pub crouch_speed: f32,
}
fn default_max_slope() -> f32 {
    0.8
}
impl MovementProfile {
    pub fn validate(self) -> Result<(), &'static str> {
        if self.shape(Action::Walk).validate().is_err()
            || !self.walk_speed.is_finite()
            || !(0.1..=12.).contains(&self.walk_speed)
            || !self.crouch_speed.is_finite()
            || !(0.1..=self.walk_speed).contains(&self.crouch_speed)
            || self
                .crouch_height
                .is_some_and(|h| !h.is_finite() || !(0.2..self.standing_height).contains(&h))
        {
            return Err("invalid navigation movement profile");
        }
        Ok(())
    }
    pub(crate) fn shape(self, action: Action) -> CharacterBody {
        CharacterBody {
            radius: self.radius,
            height: match action {
                Action::Walk => self.standing_height,
                Action::Crouch => self.crouch_height.unwrap_or(self.standing_height),
            },
            max_slope: self.max_slope,
        }
    }
    pub(crate) fn key(self) -> [u32; 4] {
        [
            self.radius.to_bits(),
            self.standing_height.to_bits(),
            self.crouch_height.unwrap_or(0.).to_bits(),
            self.max_slope.to_bits(),
        ]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Walk,
    Crouch,
}

// A bridge and the floor beneath it may occupy the same column. Source identity
// keeps them separate without rounding heights or allocating empty 3D voxels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Node {
    cell: Cell,
    surface: SurfaceSource,
}
#[derive(Clone, Copy, Debug)]
pub(crate) struct Sample {
    pub(crate) node: Node,
    pub(crate) position: Vec3,
}

type EdgeKey = ([u32; 4], Node, Cell);
type Connections = Vec<(Sample, Action, SceneryPath)>;
#[derive(Clone, Debug)]
pub struct SurfaceRoutes {
    domain: Domain,
    edges: Arc<HashMap<EdgeKey, Connections>>,
    cached_points: usize,
    stats: Stats,
}
impl SurfaceRoutes {
    pub fn new(domain: Domain) -> Result<Self, &'static str> {
        domain.validate()?;
        Ok(Self {
            domain,
            edges: Arc::default(),
            cached_points: 0,
            stats: Stats::default(),
        })
    }
    pub fn stats(&self) -> Stats {
        self.stats
    }
    pub fn domain(&self) -> Domain {
        self.domain
    }
    /// Checks support, body clearance and the final grid connector, not full route reachability.
    /// As with path search, live actors are blockers only for actor-bound agents.
    /// Callers choosing approach destinations can reject invalid endpoints before starting A*.
    pub fn destination_available(&self, world: &dyn WorldView, agent: &Agent, goal: Vec3) -> bool {
        self.finish(world, agent.profile(), goal, agent.actor())
            .is_ok()
    }
    /// Find a local destination by following support from a valid feet reference
    /// on the intended layer. Useful for approach rings around a supported target.
    /// Ignores live actors along this discovery segment, but checks the endpoint.
    /// Does not establish a route from the requesting agent to that destination.
    pub fn offset_destination(
        &self,
        world: &dyn WorldView,
        agent: &Agent,
        reference: Vec3,
        offset: [f32; 2],
    ) -> Option<Vec3> {
        let delta = Vec3::new(offset[0], offset[1], 0.);
        if !delta.finite()
            || delta.x.hypot(delta.y) > 20.
            || self.domain.cell(reference).is_none()
            || self.domain.cell(reference + delta).is_none()
        {
            return None;
        }
        let scenery = Scenery(world);
        for action in [Action::Walk, Action::Crouch] {
            if action == Action::Crouch && agent.profile().crouch_height.is_none() {
                continue;
            }
            let Ok(walk) = walk_character(
                &scenery,
                None,
                reference,
                delta,
                agent.profile().shape(action),
            ) else {
                continue;
            };
            if walk.reached && self.destination_available(world, agent, walk.position) {
                return Some(walk.position);
            }
        }
        None
    }
    pub(crate) fn connectors(
        &self,
        world: &dyn WorldView,
        profile: MovementProfile,
        from: Vec3,
        to: Cell,
    ) -> Connections {
        let target = self.domain.xy(to);
        let mut result = Vec::new();
        for action in [Action::Walk, Action::Crouch] {
            if action == Action::Crouch && profile.crouch_height.is_none() {
                continue;
            }
            // Bounded support probes follow reachable geometry, never scan an
            // entire vertical column or snap to a disconnected upper surface.
            let mut scratch = Vec::new();
            let Ok(path) = SupportStage.run(WalkRequest {
                world,
                actor: None,
                start: from,
                delta: Vec3::new(target[0] - from.x, target[1] - from.y, 0.),
                shape: profile.shape(action),
                scratch: &mut scratch,
            }) else {
                continue;
            };
            let walk = path.walk();
            let Some(support) = walk.support.filter(|_| walk.reached) else {
                continue;
            };
            let sample = Sample {
                node: Node {
                    cell: to,
                    surface: support.surface.source,
                },
                position: walk.position,
            };
            if !result
                .iter()
                .any(|(s, _, _): &(Sample, Action, SceneryPath)| s.node == sample.node)
            {
                result.push((sample, action, path.scenery_path()));
            }
        }
        result
    }
    pub(crate) fn edge(
        &mut self,
        world: &dyn WorldView,
        profile: MovementProfile,
        a: Sample,
        b: Cell,
    ) -> Connections {
        // Directed: starting layer and movement capability determine the result.
        let key = (profile.key(), a.node, b);
        if let Some(value) = self.edges.get(&key) {
            self.stats.cache_hits += 1;
            return value.clone();
        }
        self.stats.clearance_queries += 1;
        let value = self.connectors(world, profile, a.position, b);
        let points = value
            .iter()
            .map(|(_, _, path)| path.point_count())
            .sum::<usize>();
        // Bounded memoization; eviction only loses optimization, never validity.
        // Recorded paths have their own ~12 MiB coordinate budget.
        if self.edges.len() >= 262_144 || self.cached_points + points > 1_048_576 {
            self.edges = Arc::default();
            self.cached_points = 0;
        }
        if points <= 1_048_576 {
            self.cached_points += points;
            Arc::make_mut(&mut self.edges).insert(key, value.clone());
        }
        value
    }
    fn endpoints(
        &self,
        world: &dyn WorldView,
        profile: MovementProfile,
        start: Vec3,
        goal: Vec3,
        actor: Option<u64>,
    ) -> Result<Endpoints<Node, Action>, Failure> {
        let nearest = self.domain.cell(start).ok_or(Failure::Unreachable)?;
        // Occupancy is a live execution constraint, not proof that a distant
        // objective is unreachable. Approach selectors may still reject it.
        let (b, last) = self.finish(world, profile, goal, None)?;
        // A live obstacle can occupy the nearest sample. Choose another local connector,
        // rather than snapping through it or repeatedly requesting the same blocked edge.
        let mut candidates = Vec::new();
        let (w, h) = self.domain.dimensions();
        for x in nearest.0 - 1..=nearest.0 + 1 {
            for y in nearest.1 - 1..=nearest.1 + 1 {
                if x >= 0 && y >= 0 && x < w && y < h {
                    candidates.push(Cell(x, y));
                }
            }
        }
        candidates.sort_by(|a, b| {
            let horizontal = |cell| {
                let point = self.domain.xy(cell);
                (start.x - point[0]).hypot(start.y - point[1])
            };
            horizontal(*a).total_cmp(&horizontal(*b)).then(a.cmp(b))
        });
        let mut failure = Failure::Unreachable;
        for a in candidates {
            for (sample, action, path) in self.connectors(world, profile, start, a) {
                let point = sample.position;
                if !path
                    .clear(world, actor, start, point, profile.shape(action))
                    .unwrap_or_else(|| {
                        actor.is_none_or(|id| {
                            live_supported_segment(world, id, profile.shape(action), start, point)
                        })
                    })
                {
                    failure = Failure::Blocked;
                    continue;
                }
                let first = Waypoint {
                    position: point,
                    action,
                };
                let a = sample.node;
                return Ok(Endpoints {
                    start: Location {
                        node: a,
                        position: point,
                    },
                    goal: b,
                    first,
                    last,
                });
            }
        }
        Err(failure)
    }
    fn finish(
        &self,
        world: &dyn WorldView,
        profile: MovementProfile,
        goal: Vec3,
        actor: Option<u64>,
    ) -> Result<(Node, Waypoint), Failure> {
        let cell = self.domain.cell(goal).ok_or(Failure::Unreachable)?;
        let mut failure = Failure::Unreachable;
        for (sample, _, _) in self.connectors(world, profile, goal, cell) {
            let point = sample.position;
            let Some(action) = connection(world, profile, point, goal) else {
                continue;
            };
            if actor.is_some_and(|id| {
                !live_supported_segment(world, id, profile.shape(action), point, goal)
            }) {
                failure = Failure::Blocked;
                continue;
            }
            return Ok((
                sample.node,
                Waypoint {
                    position: goal,
                    action,
                },
            ));
        }
        Err(failure)
    }
}
impl RouteProvider for SurfaceRoutes {
    type Config = Domain;
    fn goal_tolerance(&self) -> f32 {
        self.domain.cell_size * 0.5
    }
    type Node = Node;
    type Action = Action;
    type Profile = MovementProfile;
    fn new(domain: Domain) -> Result<Self, &'static str> {
        Self::new(domain)
    }
    fn validate_profile(profile: MovementProfile) -> Result<(), &'static str> {
        profile.validate()
    }
    fn invalidate(&mut self) {
        self.edges = Arc::default();
        self.cached_points = 0;
    }
    fn route_validity(&self) -> RouteValidity {
        RouteValidity::LiveChecked
    }
    fn goal_approach_radius(&self, profile: MovementProfile) -> f32 {
        profile.radius * 2. + self.domain.cell_size * std::f32::consts::SQRT_2
    }
    fn begin(
        &self,
        w: &dyn WorldView,
        p: MovementProfile,
        start: Vec3,
        goal: Vec3,
        actor: Option<u64>,
    ) -> Result<Endpoints<Node, Action>, Failure> {
        self.endpoints(w, p, start, goal, actor)
    }
    fn destination_available(
        &self,
        w: &dyn WorldView,
        p: MovementProfile,
        actor: Option<u64>,
        goal: Vec3,
    ) -> bool {
        self.finish(w, p, goal, actor).is_ok()
    }
    fn neighbors(
        &mut self,
        w: &dyn WorldView,
        p: MovementProfile,
        from: Location<Node>,
        stats: &mut Stats,
    ) -> Vec<Connection<Node, Action>> {
        let at = from.node.cell;
        let (width, height) = self.domain.dimensions();
        let mut result = vec![];
        let before = self.stats;
        for cell in [
            Cell(at.0 - 1, at.1),
            Cell(at.0 + 1, at.1),
            Cell(at.0, at.1 - 1),
            Cell(at.0, at.1 + 1),
            Cell(at.0 - 1, at.1 - 1),
            Cell(at.0 - 1, at.1 + 1),
            Cell(at.0 + 1, at.1 - 1),
            Cell(at.0 + 1, at.1 + 1),
        ] {
            if cell.0 < 0 || cell.1 < 0 || cell.0 >= width || cell.1 >= height {
                continue;
            }
            for (sample, action, _) in self.edge(
                w,
                p,
                Sample {
                    node: from.node,
                    position: from.position,
                },
                cell,
            ) {
                let length = if at.0 != cell.0 && at.1 != cell.1 {
                    1414
                } else {
                    1000
                };
                let cost = match action {
                    Action::Walk => length,
                    Action::Crouch => (length as f32 * p.walk_speed / p.crouch_speed).ceil() as u64,
                };
                result.push(Connection {
                    destination: Location {
                        node: sample.node,
                        position: sample.position,
                    },
                    action,
                    cost,
                });
            }
        }
        stats.cache_hits += self.stats.cache_hits - before.cache_hits;
        stats.clearance_queries += self.stats.clearance_queries - before.clearance_queries;
        result
    }
    fn heuristic(&self, from: Node, goal: Node) -> u64 {
        let dx = (from.cell.0 - goal.cell.0).unsigned_abs() as u64;
        let dy = (from.cell.1 - goal.cell.1).unsigned_abs() as u64;
        dx.min(dy) * 1414 + dx.abs_diff(dy) * 1000
    }
    fn live_clear(
        &self,
        w: &dyn WorldView,
        p: MovementProfile,
        actor: Option<u64>,
        a: Vec3,
        b: Vec3,
        action: Action,
    ) -> bool {
        actor.is_none_or(|id| live_supported_segment(w, id, p.shape(action), a, b))
    }
    fn connection_clear(
        &self,
        world: &dyn WorldView,
        profile: MovementProfile,
        actor: Option<u64>,
        from: Location<Node>,
        edge: &Connection<Node, Action>,
    ) -> bool {
        let key = (profile.key(), from.node, edge.destination.node.cell);
        self.edges
            .get(&key)
            .and_then(|paths| {
                paths.iter().find(|(s, action, _)| {
                    s.node == edge.destination.node && *action == edge.action
                })
            })
            .and_then(|(_, _, path)| {
                path.clear(
                    world,
                    actor,
                    from.position,
                    edge.destination.position,
                    profile.shape(edge.action),
                )
            })
            .unwrap_or_else(|| {
                self.live_clear(
                    world,
                    profile,
                    actor,
                    from.position,
                    edge.destination.position,
                    edge.action,
                )
            })
    }
    fn automatic_completion(&self, _: Action) -> bool {
        true
    }
    fn segment_clear(
        &self,
        w: &dyn WorldView,
        p: MovementProfile,
        _actor: Option<u64>,
        a: Vec3,
        b: Vec3,
        action: Action,
    ) -> bool {
        // Route look-ahead describes a corridor, not occupancy several meters
        // ahead. The executor's immediate steering check includes every actor.
        traversable(w, p, None, a, b, action)
    }
    fn steering_clear(
        &self,
        w: &dyn WorldView,
        p: MovementProfile,
        actor: Option<u64>,
        a: Vec3,
        b: Vec3,
        action: Action,
    ) -> bool {
        if distance(a, b) > 6.
            || self.domain.cell(a).is_none()
            || self.domain.cell(b).is_none()
            || (action == Action::Crouch && p.crouch_height.is_none())
        {
            return false;
        }
        let scenery = Scenery(w);
        let view: &dyn WorldView = if actor.is_some() { w } else { &scenery };
        walk_character(
            view,
            actor,
            a,
            Vec3::new(b.x - a.x, b.y - a.y, 0.),
            p.shape(action),
        )
        .is_ok_and(|v| v.reached)
    }
    fn offset_destination(
        &self,
        w: &dyn WorldView,
        p: MovementProfile,
        actor: Option<u64>,
        reference: Vec3,
        offset: [f32; 2],
    ) -> Option<Vec3> {
        let agent = match actor {
            Some(id) => Agent::for_actor(id, p, reference),
            None => Agent::new(p, reference),
        }
        .ok()?;
        self.offset_destination(w, &agent, reference, offset)
    }
    fn familiarize(
        &mut self,
        w: &dyn WorldView,
        p: MovementProfile,
        point: Vec3,
        _: &mut Stats,
    ) -> Vec<Node> {
        let Some(cell) = self.domain.cell(point) else {
            return vec![];
        };
        self.connectors(w, p, point, cell)
            .into_iter()
            .map(|(s, _, _)| s.node)
            .collect()
    }
}
// Actors are transient blockers, never baked into shared world knowledge. Execution still
// uses the full WorldView and must stop/replan on live collisions.
pub(crate) struct Scenery<'a>(pub(crate) &'a dyn WorldView);
impl WorldView for Scenery<'_> {
    fn space(&self) -> &Space {
        self.0.space()
    }
    fn items(&self) -> &[Item] {
        self.0.items()
    }
    fn item(&self, id: u64) -> Option<&Item> {
        self.0.item(id)
    }
    fn query(&self, p: Vec3, r: f32) -> Vec<usize> {
        self.0
            .query(p, r)
            .into_iter()
            .filter(|&i| self.0.items()[i].character_body.is_none())
            .collect()
    }
    fn terrain(&self) -> Option<&io_world::HeightField> {
        self.0.terrain()
    }
    fn revision(&self) -> u64 {
        self.0.revision()
    }
    fn spatial_revision(&self) -> u64 {
        self.0.spatial_revision()
    }
    fn physics_stats(&self) -> PhysicsStats {
        self.0.physics_stats()
    }
    fn physics_error(&self) -> Option<&str> {
        self.0.physics_error()
    }
}
pub(crate) fn connection(
    world: &dyn WorldView,
    profile: MovementProfile,
    a: Vec3,
    b: Vec3,
) -> Option<Action> {
    [Action::Walk, Action::Crouch]
        .into_iter()
        .find(|&action| supported_segment(world, profile, a, b, action))
}
pub(crate) fn supported_segment(
    world: &dyn WorldView,
    profile: MovementProfile,
    a: Vec3,
    b: Vec3,
    action: Action,
) -> bool {
    if action == Action::Crouch && profile.crouch_height.is_none() {
        return false;
    }
    let scenery = Scenery(world);
    let shape = profile.shape(action);
    character_supported_segment(&scenery, None, a, b, shape)
}
fn traversable(
    world: &dyn WorldView,
    profile: MovementProfile,
    actor: Option<u64>,
    a: Vec3,
    b: Vec3,
    action: Action,
) -> bool {
    if action == Action::Crouch && profile.crouch_height.is_none() {
        return false;
    }
    crate::pipeline::segment_clear(world, actor, a, b, profile.shape(action))
}
pub(crate) fn live_supported_segment(
    world: &dyn WorldView,
    actor: u64,
    shape: CharacterBody,
    a: Vec3,
    b: Vec3,
) -> bool {
    crate::pipeline::segment_clear(world, Some(actor), a, b, shape)
}
