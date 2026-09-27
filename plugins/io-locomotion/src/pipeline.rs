//! Short-lived movement proposals bound to one borrowed world and body/request.
//! No global cache, worker or new movement authority. Application stays in Motor.
use crate::surface::Scenery;
use io_game::stage::{Stage, Then};
use io_types::Vec3;
use io_world::{
    character_path_clear_of_actors, trace_character_walk, CharacterBody, CharacterWalk, WorldView,
};

#[derive(Default, Debug)]
pub(crate) struct WalkScratch(pub Vec<Vec3>);
impl Clone for WalkScratch {
    fn clone(&self) -> Self {
        // Published game clones must not copy per-call working buffers.
        Self::default()
    }
}

pub struct WalkRequest<'a> {
    pub world: &'a dyn WorldView,
    pub actor: Option<u64>,
    pub start: Vec3,
    pub delta: Vec3,
    pub shape: CharacterBody,
    /// Caller-owned temporary storage; cleared on every request, never a cache.
    pub scratch: &'a mut Vec<Vec3>,
}

#[derive(Debug, PartialEq)]
pub enum WalkError {
    Invalid(String),
    ActorBlocked,
    UnsupportedStart,
}

/// Private provenance prevents substituting a different world/body between stages.
/// The immutable world borrow prevents safe callers mutating it during validation.
///
/// ```compile_fail
/// use io_game::stage::Stage;
/// use io_locomotion::pipeline::{SupportStage, ActorClearanceStage, WalkRequest};
/// use io_world::{World, Space, CharacterBody};
/// use io_types::Vec3;
/// let mut world = World::new(Space::new(Vec3::new(10.,10.,10.)), vec![]);
/// let mut scratch = vec![];
/// let path = SupportStage.run(WalkRequest {
///     world: &world, actor: None, start: Vec3::default(), delta: Vec3::default(),
///     shape: CharacterBody { radius:0.3, height:1.8, max_slope:0.8 }, scratch:&mut scratch,
/// }).unwrap();
/// world.set_pose(1, Vec3::new(1.,0.,0.), 0.); // cannot mutate the borrowed world
/// let _ = ActorClearanceStage.run(path);
/// ```
pub struct SupportedTrajectory<'a> {
    world: &'a dyn WorldView,
    actor: Option<u64>,
    shape: CharacterBody,
    points: &'a [Vec3],
    walk: CharacterWalk,
}
impl SupportedTrajectory<'_> {
    pub fn points(&self) -> &[Vec3] {
        self.points
    }
    pub fn walk(&self) -> CharacterWalk {
        self.walk
    }
}

/// Safe prefix against scenery and current actors, not necessarily the entire request.
/// Inspect `reached` before claiming arrival. Do not retain as a cross-tick permission.
pub struct ValidatedMovement<'a>(SupportedTrajectory<'a>);
impl ValidatedMovement<'_> {
    pub fn points(&self) -> &[Vec3] {
        self.0.points
    }
    pub fn into_walk(self) -> CharacterWalk {
        self.0.walk
    }
}

pub struct SupportStage;
impl<'a> Stage<WalkRequest<'a>> for SupportStage {
    type Output = SupportedTrajectory<'a>;
    type Error = WalkError;
    fn run(&mut self, input: WalkRequest<'a>) -> Result<Self::Output, Self::Error> {
        let walk = trace_character_walk(
            &Scenery(input.world),
            input.actor,
            input.start,
            input.delta,
            input.shape,
            input.scratch,
        )
        .map_err(WalkError::Invalid)?;
        if input.scratch.is_empty() {
            return Err(WalkError::UnsupportedStart);
        }
        Ok(SupportedTrajectory {
            world: input.world,
            actor: input.actor,
            shape: input.shape,
            points: input.scratch,
            walk,
        })
    }
}

pub struct ActorClearanceStage;
impl<'a> Stage<SupportedTrajectory<'a>> for ActorClearanceStage {
    type Output = ValidatedMovement<'a>;
    type Error = WalkError;
    fn run(&mut self, input: SupportedTrajectory<'a>) -> Result<Self::Output, Self::Error> {
        if !character_path_clear_of_actors(input.world, input.actor, input.points, input.shape) {
            return Err(WalkError::ActorBlocked);
        }
        Ok(ValidatedMovement(input))
    }
}

/// Actor-less planning intentionally excludes transient actors from shared routes.
pub(crate) fn walk(
    world: &dyn WorldView,
    actor: Option<u64>,
    start: Vec3,
    delta: Vec3,
    shape: CharacterBody,
    scratch: &mut Vec<Vec3>,
) -> Result<CharacterWalk, WalkError> {
    let request = WalkRequest {
        world,
        actor,
        start,
        delta,
        shape,
        scratch,
    };
    match actor {
        Some(_) => Then(SupportStage, ActorClearanceStage)
            .run(request)
            .map(ValidatedMovement::into_walk),
        None => SupportStage.run(request).map(|v| v.walk()),
    }
}

pub(crate) fn segment_clear(
    world: &dyn WorldView,
    actor: Option<u64>,
    start: Vec3,
    end: Vec3,
    shape: CharacterBody,
) -> bool {
    end.finite()
        && walk(
            world,
            actor,
            start,
            Vec3::new(end.x - start.x, end.y - start.y, 0.),
            shape,
            &mut Vec::new(),
        )
        .is_ok_and(|v| v.reached && (v.position.z - end.z).abs() <= 0.003)
}
