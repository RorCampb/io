//! Typed route connections. Providers own capabilities and action meaning.
use super::{Stats, Waypoint};
use io_types::Vec3;
use io_world::WorldView;
use std::{fmt::Debug, hash::Hash};

#[derive(Clone, Copy, Debug)]
pub struct Location<N> {
    pub node: N,
    pub position: Vec3,
}
#[derive(Clone, Copy, Debug)]
pub struct Connection<N, A> {
    pub destination: Location<N>,
    pub action: A,
    pub cost: u64,
}
#[derive(Clone, Copy, Debug)]
pub enum Failure {
    Unreachable,
    Blocked,
}

/// Cache revisions always invalidate search inputs. They need not cancel an
/// accepted continuous route whose provider/executor checks each live movement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteValidity {
    RevisionBound,
    LiveChecked,
}
#[derive(Clone, Debug)]
/// Explicit entry/exit actions surround the graph connections, even when their
/// positions coincide with graph nodes. Discrete actions are not skipped.
pub struct Endpoints<N, A> {
    pub start: Location<N>,
    pub goal: N,
    pub first: Waypoint<A>,
    pub last: Waypoint<A>,
}

/// Trusted Rust implementation. Calls must be deterministic, bounded and nonblocking.
/// A node identity must resolve to a stable position for one profile/revision.
/// Costs must be positive; heuristic must be admissible (zero is always safe).
pub trait RouteProvider: Clone + Debug + Send + Sync + 'static {
    type Config: Clone + Debug + Send + Sync;
    type Node: Copy + Debug + Eq + Ord + Hash + Send + Sync;
    type Action: Copy + Debug + Eq + Send + Sync;
    type Profile: Copy + Debug + PartialEq + Send + Sync;
    fn new(config: Self::Config) -> Result<Self, &'static str>;
    fn goal_tolerance(&self) -> f32 {
        0.25
    }
    fn validate_profile(profile: Self::Profile) -> Result<(), &'static str>;
    fn invalidate(&mut self);
    fn route_validity(&self) -> RouteValidity {
        RouteValidity::RevisionBound
    }
    /// A temporary occupant need not prevent approaching a distant destination.
    /// Opt-in executors must still stop before actual contact.
    fn goal_approach_radius(&self, _profile: Self::Profile) -> f32 {
        0.
    }
    /// Revision of persistent planning inputs. Transient blockers still need live checks.
    fn revision(&self, world: &dyn WorldView) -> u64 {
        world.navigation_revision()
    }
    fn begin(
        &self,
        world: &dyn WorldView,
        profile: Self::Profile,
        start: Vec3,
        goal: Vec3,
        actor: Option<u64>,
    ) -> Result<Endpoints<Self::Node, Self::Action>, Failure>;
    fn destination_available(
        &self,
        world: &dyn WorldView,
        profile: Self::Profile,
        actor: Option<u64>,
        goal: Vec3,
    ) -> bool;
    /// Persistent eligible connections; return at most 64 per expansion.
    /// Transient blockers belong in live_clear rather than a shared scenery cache.
    fn neighbors(
        &mut self,
        world: &dyn WorldView,
        profile: Self::Profile,
        from: Location<Self::Node>,
        stats: &mut Stats,
    ) -> Vec<Connection<Self::Node, Self::Action>>;
    fn heuristic(&self, _from: Self::Node, _goal: Self::Node) -> u64 {
        0
    }
    /// A false result excludes this connection on the first search pass. A second
    /// pass may ignore it solely to distinguish Blocked from Unreachable; that
    /// second pass never produces an executable route.
    fn live_clear(
        &self,
        world: &dyn WorldView,
        profile: Self::Profile,
        actor: Option<u64>,
        from: Vec3,
        to: Vec3,
        action: Self::Action,
    ) -> bool;
    /// Check transient constraints on a connection returned by `neighbors` for
    /// this world/profile/source. Providers may reuse its geometry evidence.
    /// The default preserves existing plugins' complete validation behavior.
    fn connection_clear(
        &self,
        world: &dyn WorldView,
        profile: Self::Profile,
        actor: Option<u64>,
        from: Location<Self::Node>,
        edge: &Connection<Self::Node, Self::Action>,
    ) -> bool {
        self.live_clear(
            world,
            profile,
            actor,
            from.position,
            edge.destination.position,
            edge.action,
        )
    }
    /// Opt in only for continuous travel whose completion is positional.
    /// Discrete plugin actions require explicit executor acknowledgement.
    fn automatic_completion(&self, _action: Self::Action) -> bool {
        false
    }
    fn segment_clear(
        &self,
        world: &dyn WorldView,
        profile: Self::Profile,
        actor: Option<u64>,
        from: Vec3,
        to: Vec3,
        action: Self::Action,
    ) -> bool;
    fn steering_clear(
        &self,
        world: &dyn WorldView,
        profile: Self::Profile,
        actor: Option<u64>,
        from: Vec3,
        to: Vec3,
        action: Self::Action,
    ) -> bool {
        self.segment_clear(world, profile, actor, from, to, action)
    }
    fn offset_destination(
        &self,
        _world: &dyn WorldView,
        _profile: Self::Profile,
        _actor: Option<u64>,
        _reference: Vec3,
        _offset: [f32; 2],
    ) -> Option<Vec3> {
        None
    }
    fn familiarize(
        &mut self,
        _world: &dyn WorldView,
        _profile: Self::Profile,
        _point: Vec3,
        _stats: &mut Stats,
    ) -> Vec<Self::Node> {
        vec![]
    }
}
