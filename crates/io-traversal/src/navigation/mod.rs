#![forbid(unsafe_code)]
//! Budgeted route search and progress. Action meaning and geometry belong to providers.
use io_types::Vec3;
use io_world::WorldView;
use serde::Deserialize;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet, VecDeque};
use std::sync::Arc;
mod provider;
pub use provider::*;
mod refinement;
pub use refinement::{
    HandoffRequest, RefineProgress, RefineRequest, RouteHandoff, RouteRefinement,
};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Cell(pub i32, pub i32);
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Domain {
    /// XY sampling origin. Z is retained for scene compatibility, not a floor constraint.
    pub origin: [f32; 3],
    pub size: [f32; 2],
    pub cell_size: f32,
}
impl Domain {
    pub fn validate(self) -> Result<(), &'static str> {
        if self
            .origin
            .iter()
            .chain(self.size.iter())
            .any(|v| !v.is_finite() || v.abs() > 100_000.)
            || !self.cell_size.is_finite()
            || !(0.2..=2.).contains(&self.cell_size)
            || self.size.iter().any(|v| *v < self.cell_size)
            || self.dimensions().0 as u64 * self.dimensions().1 as u64 > 65_536
        {
            return Err("invalid navigation domain (maximum 65536 cells)");
        }
        Ok(())
    }
    pub fn dimensions(self) -> (i32, i32) {
        (
            (self.size[0] / self.cell_size).floor() as i32 + 1,
            (self.size[1] / self.cell_size).floor() as i32 + 1,
        )
    }
    pub fn xy(self, c: Cell) -> [f32; 2] {
        [
            self.origin[0] + c.0 as f32 * self.cell_size,
            self.origin[1] + c.1 as f32 * self.cell_size,
        ]
    }
    pub fn cell(self, p: Vec3) -> Option<Cell> {
        if !p.finite()
            || p.x < self.origin[0]
            || p.y < self.origin[1]
            || p.x > self.origin[0] + self.size[0]
            || p.y > self.origin[1] + self.size[1]
        {
            return None;
        }
        let (w, h) = self.dimensions();
        Some(Cell(
            ((p.x - self.origin[0]) / self.cell_size)
                .round()
                .min((w - 1) as f32) as i32,
            ((p.y - self.origin[1]) / self.cell_size)
                .round()
                .min((h - 1) as f32) as i32,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Planning,
    Following,
    Arrived,
    Unreachable,
    Blocked,
    Failed,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Waypoint<A> {
    pub position: Vec3,
    pub action: A,
}
#[derive(Clone, Debug)]
pub struct Knowledge<N: Copy + Eq + std::hash::Hash> {
    revision: Option<u64>,
    cells: Arc<HashSet<N>>,
}
impl<N: Copy + Eq + std::hash::Hash> Default for Knowledge<N> {
    fn default() -> Self {
        Self {
            revision: None,
            cells: Arc::default(),
        }
    }
}
impl<N: Copy + Eq + std::hash::Hash> Knowledge<N> {
    pub fn known_cells(&self) -> usize {
        self.cells.len()
    }
    fn learn(&mut self, revision: u64, cell: N) {
        if self.revision != Some(revision) {
            self.cells = Arc::default();
            self.revision = Some(revision);
        }
        if !self.cells.contains(&cell) {
            Arc::make_mut(&mut self.cells).insert(cell);
        }
    }
}

#[derive(Clone, Debug)]
struct Search<P: RouteProvider> {
    open: BinaryHeap<Reverse<(u64, u64, P::Node)>>,
    cost: HashMap<P::Node, u64>,
    parents: HashMap<P::Node, (P::Node, P::Action)>,
    positions: HashMap<P::Node, Vec3>,
    start: P::Node,
    goal: P::Node,
    first: Waypoint<P::Action>,
    last: Waypoint<P::Action>,
    transient_blocker: bool,
    mode: SearchMode,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SearchMode {
    AvoidActors,
    ConfirmReachability,
}

#[derive(Clone, Debug)]
enum State<P: RouteProvider> {
    New,
    Searching(Arc<Search<P>>),
    Route {
        steps: Arc<[Waypoint<P::Action>]>,
        next: usize,
        refined: bool,
    },
    Arrived,
    Unreachable,
    Blocked,
    Failed,
}
#[derive(Clone, Debug)]
pub struct Agent<P: RouteProvider> {
    actor: Option<u64>,
    profile: P::Profile,
    goal: Vec3,
    revision: Option<u64>,
    state: State<P>,
    knowledge: Knowledge<P::Node>,
}
impl<P: RouteProvider> Agent<P> {
    pub fn new(profile: P::Profile, goal: Vec3) -> Result<Self, &'static str> {
        P::validate_profile(profile)?;
        if !goal.finite() {
            return Err("invalid navigation goal");
        }
        Ok(Self {
            actor: None,
            profile,
            goal,
            revision: None,
            state: State::New,
            knowledge: Knowledge::default(),
        })
    }
    pub fn for_actor(actor: u64, profile: P::Profile, goal: Vec3) -> Result<Self, &'static str> {
        let mut agent = Self::new(profile, goal)?;
        agent.actor = Some(actor);
        Ok(agent)
    }
    pub fn status(&self) -> Status {
        match self.state {
            State::New | State::Searching(_) => Status::Planning,
            State::Route { .. } => Status::Following,
            State::Arrived => Status::Arrived,
            State::Unreachable => Status::Unreachable,
            State::Blocked => Status::Blocked,
            State::Failed => Status::Failed,
        }
    }
    pub fn knowledge(&self) -> &Knowledge<P::Node> {
        &self.knowledge
    }
    /// Read-only planned actions for inspection. Route ownership/progress stays here.
    pub fn route(&self) -> Option<&[Waypoint<P::Action>]> {
        match &self.state {
            State::Route { steps, .. } => Some(steps),
            _ => None,
        }
    }
    pub fn remaining_route(&self) -> Option<&[Waypoint<P::Action>]> {
        match &self.state {
            State::Route { steps, next, .. } => Some(&steps[*next..]),
            _ => None,
        }
    }
    pub fn retry(&mut self) {
        self.state = State::New;
    }
    pub fn set_profile(&mut self, profile: P::Profile) -> Result<(), &'static str> {
        P::validate_profile(profile)?;
        if self.profile != profile {
            self.profile = profile;
            self.retry();
        }
        Ok(())
    }
    /// Retarget without discarding discovered scenery. Failed validation preserves the old goal.
    pub fn set_goal(&mut self, goal: Vec3) -> Result<(), &'static str> {
        if !goal.finite() {
            return Err("invalid navigation goal");
        }
        self.goal = goal;
        self.retry();
        Ok(())
    }
    pub fn profile(&self) -> P::Profile {
        self.profile
    }
    pub fn actor(&self) -> Option<u64> {
        self.actor
    }
    /// Acknowledge only the currently active step after successful execution.
    pub fn complete_step(&mut self) {
        if let State::Route { steps, next, .. } = &mut self.state {
            *next += 1;
            if *next == steps.len() {
                self.state = State::Arrived;
            }
        }
    }
    pub fn block(&mut self) {
        self.state = State::Blocked;
    }
    pub fn fail(&mut self) {
        self.state = State::Failed;
    }
    pub(crate) fn unreachable(&mut self) {
        self.state = State::Unreachable;
    }
}

#[derive(Clone, Copy, Debug, Default, serde::Serialize)]
pub struct Stats {
    pub expanded: u64,
    pub clearance_queries: u64,
    pub cache_hits: u64,
    pub invalidations: u64,
    /// Live route-shortcut/steering checks, separate from A* expansion work.
    pub steering_queries: u64,
    /// Snapshot refinement checks, not repeated by the live follower.
    pub refinement_queries: u64,
}

#[derive(Clone, Debug)]
pub struct Navigation<P: RouteProvider> {
    config: P::Config,
    provider: P,
    revision: Option<u64>,
    stats: Stats,
}
impl<P: RouteProvider> Navigation<P> {
    pub fn new(config: P::Config) -> Result<Self, &'static str> {
        Ok(Self {
            provider: P::new(config.clone())?,
            config,
            revision: None,
            stats: Stats::default(),
        })
    }
    pub fn configuration(&self) -> P::Config {
        self.config.clone()
    }
    pub fn goal_tolerance(&self) -> f32 {
        self.provider.goal_tolerance()
    }
    pub fn stats(&self) -> Stats {
        self.stats
    }
    pub(crate) fn add_stats(&mut self, s: Stats) {
        self.stats.expanded += s.expanded;
        self.stats.clearance_queries += s.clearance_queries;
        self.stats.cache_hits += s.cache_hits;
        self.stats.invalidations += s.invalidations;
        self.stats.steering_queries += s.steering_queries;
        self.stats.refinement_queries += s.refinement_queries;
    }
    pub fn revision(&self, world: &dyn WorldView) -> u64 {
        self.provider.revision(world)
    }
    pub(crate) fn live_checked(&self) -> bool {
        self.provider.route_validity() == RouteValidity::LiveChecked
    }
    /// A continuous replacement must connect from the *current* pose, not its
    /// worker snapshot's start. The caller checks the result's world revision;
    /// discrete actions keep their exact takeoff contract here.
    pub(crate) fn accept_handoff(
        &mut self,
        world: &dyn WorldView,
        agent: &mut Agent<P>,
        planned_start: Vec3,
        position: Vec3,
        horizon: f32,
    ) -> bool {
        let revision = self.revision(world);
        if !self.live_checked() {
            let accepted = distance(planned_start, position) <= 0.01;
            if accepted {
                agent.revision = Some(revision);
            }
            return accepted;
        }
        let State::Route { steps, next, .. } = &mut agent.state else {
            let accepted = distance(planned_start, position) <= 0.01;
            if accepted {
                agent.revision = Some(revision);
            }
            return accepted;
        };
        if steps
            .iter()
            .any(|s| !self.provider.automatic_completion(s.action))
        {
            return false;
        }
        // Bound connector work even for very long routes. Never splice across a
        // wall or onto a disconnected vertical layer.
        for index in (*next..steps.len().min(*next + 32))
            .rev()
            .filter(|&i| distance(position, steps[i].position) <= horizon)
            .take(8)
        {
            let step = steps[index];
            self.stats.steering_queries += 1;
            if self.provider.segment_clear(
                world,
                agent.profile,
                agent.actor,
                position,
                step.position,
                step.action,
            ) {
                *next = index;
                agent.revision = Some(revision);
                return true;
            }
        }
        false
    }
    fn synchronize(&mut self, revision: u64) {
        if self.revision != Some(revision) {
            self.stats.invalidations += u64::from(self.revision.is_some());
            self.provider.invalidate();
            self.revision = Some(revision);
        }
    }
    pub fn destination_available(
        &self,
        world: &dyn WorldView,
        agent: &Agent<P>,
        goal: Vec3,
    ) -> bool {
        goal.finite()
            && self
                .provider
                .destination_available(world, agent.profile, agent.actor, goal)
    }
    pub fn offset_destination(
        &self,
        world: &dyn WorldView,
        agent: &Agent<P>,
        reference: Vec3,
        offset: [f32; 2],
    ) -> Option<Vec3> {
        self.provider
            .offset_destination(world, agent.profile, agent.actor, reference, offset)
            .filter(|p| p.finite())
    }
    pub fn familiarize(&mut self, world: &dyn WorldView, agent: &mut Agent<P>, points: &[Vec3]) {
        self.synchronize(self.revision(world));
        for point in points.iter().take(256).filter(|p| p.finite()) {
            for node in self
                .provider
                .familiarize(world, agent.profile, *point, &mut self.stats)
                .into_iter()
                .take(64)
            {
                agent.knowledge.learn(self.revision(world), node);
            }
        }
    }
    /// At most `budget` frontier entries are processed. Movement/execution remains with the caller.
    /// A zero budget still permits following an already-computed path.
    pub fn advance(
        &mut self,
        world: &dyn WorldView,
        agent: &mut Agent<P>,
        position: Vec3,
        budget: usize,
    ) -> Option<Waypoint<P::Action>> {
        let revision = self.revision(world);
        self.synchronize(revision);
        if agent.revision != Some(revision) {
            if !self.live_checked() || matches!(agent.state, State::Searching(_)) {
                agent.state = State::New;
            }
            agent.revision = Some(revision);
            if agent.knowledge.revision != Some(revision) {
                agent.knowledge = Knowledge::default();
            }
        }
        if matches!(agent.state, State::New) && budget > 0 {
            agent.state =
                match self
                    .provider
                    .begin(world, agent.profile, position, agent.goal, agent.actor)
                {
                    Ok(e)
                        if e.start.position.finite()
                            && e.first.position.finite()
                            && e.last.position.finite() =>
                    {
                        State::Searching(Arc::new(Search {
                            open: BinaryHeap::from([Reverse((0, 0, e.start.node))]),
                            cost: HashMap::from([(e.start.node, 0)]),
                            parents: HashMap::new(),
                            positions: HashMap::from([(e.start.node, e.start.position)]),
                            start: e.start.node,
                            goal: e.goal,
                            first: e.first,
                            last: e.last,
                            transient_blocker: false,
                            mode: SearchMode::AvoidActors,
                        }))
                    }
                    Ok(_) => State::Failed,
                    Err(Failure::Blocked) => State::Blocked,
                    Err(Failure::Unreachable) => State::Unreachable,
                };
        }
        if let State::Searching(search) = &mut agent.state {
            let search = Arc::make_mut(search);
            for _ in 0..budget.min(4096) {
                let Some(Reverse((_, cost, cell))) = search.open.pop() else {
                    if search.transient_blocker && search.mode == SearchMode::AvoidActors {
                        // Distinguish a genuinely impossible route from temporary actor traffic.
                        search.mode = SearchMode::ConfirmReachability;
                        search.open.push(Reverse((0, 0, search.start)));
                        search.cost.clear();
                        search.cost.insert(search.start, 0);
                        search.parents.clear();
                        continue;
                    }
                    agent.state = State::Unreachable;
                    break;
                };
                if search.cost.get(&cell) != Some(&cost) {
                    continue;
                }
                self.stats.expanded += 1;
                agent.knowledge.learn(revision, cell);
                if cell == search.goal {
                    if search.mode == SearchMode::ConfirmReachability {
                        agent.state = State::Blocked;
                        break;
                    }
                    let mut path = VecDeque::from([search.last]);
                    let mut c = cell;
                    while c != search.start {
                        let (parent, action) = search.parents[&c];
                        path.push_front(Waypoint {
                            position: search.positions[&c],
                            action,
                        });
                        c = parent;
                    }
                    path.push_front(search.first);
                    agent.state = State::Route {
                        steps: Vec::from(path).into(),
                        next: 0,
                        refined: false,
                    };
                    break;
                }

                let from = Location {
                    node: cell,
                    position: search.positions[&cell],
                };
                let neighbors =
                    self.provider
                        .neighbors(world, agent.profile, from, &mut self.stats);
                // A bounded contract, not silently truncated connectivity.
                if neighbors.len() > 64
                    || neighbors
                        .iter()
                        .any(|n| n.cost == 0 || !n.destination.position.finite())
                {
                    agent.state = State::Failed;
                    break;
                }
                for edge in neighbors {
                    let next = edge.destination.node;
                    agent.knowledge.learn(revision, next);
                    let approach = self.provider.goal_approach_radius(agent.profile);
                    if search.mode == SearchMode::AvoidActors
                        && !(approach.is_finite()
                            && approach > 0.
                            && distance(edge.destination.position, search.last.position)
                                <= approach)
                        && !self.provider.live_clear(
                            world,
                            agent.profile,
                            agent.actor,
                            from.position,
                            edge.destination.position,
                            edge.action,
                        )
                    {
                        search.transient_blocker = true;
                        continue;
                    }
                    let Some(candidate) = cost.checked_add(edge.cost) else {
                        continue;
                    };
                    if search.cost.get(&next).is_some_and(|old| *old <= candidate) {
                        continue;
                    }
                    search.cost.insert(next, candidate);
                    search.positions.insert(next, edge.destination.position);
                    search.parents.insert(next, (cell, edge.action));
                    let heuristic = self.provider.heuristic(next, search.goal);
                    search.open.push(Reverse((
                        candidate.saturating_add(heuristic),
                        candidate,
                        next,
                    )));
                }
            }
        }
        if let State::Route { steps, next, .. } = &mut agent.state {
            while steps.get(*next).is_some_and(|p| {
                self.provider.automatic_completion(p.action)
                    && distance(p.position, position) < 0.015
            }) {
                // Arrival tolerance must not cut a tight corner. A connector was
                // proved from the waypoint, not every point within its arrival ball.
                if let Some(following) = steps.get(*next + 1) {
                    self.stats.steering_queries += 1;
                    if !self.provider.segment_clear(
                        world,
                        agent.profile,
                        agent.actor,
                        position,
                        following.position,
                        following.action,
                    ) {
                        break;
                    }
                }
                *next += 1;
            }
            match steps.get(*next) {
                Some(next) => return Some(*next),
                None => agent.state = State::Arrived,
            }
        }
        None
    }
    /// Delegate validation of immediate execution intent to the route provider.
    /// The provider determines whether this follows support or travels freely in 3D.
    pub fn steering_segment_clear(
        &mut self,
        world: &dyn WorldView,
        agent: &Agent<P>,
        a: Vec3,
        b: Vec3,
        action: P::Action,
    ) -> bool {
        self.stats.steering_queries += 1;
        a.finite()
            && b.finite()
            && self
                .provider
                .steering_clear(world, agent.profile, agent.actor, a, b, action)
    }
}
fn distance(a: Vec3, b: Vec3) -> f32 {
    let d = a - b;
    d.dot(d).sqrt()
}
