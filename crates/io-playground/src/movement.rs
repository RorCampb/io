//! Application composition: existing locomotion navigation or the athletics example.
use crate::{athletics, Error};
use io_game::PluginWorld;
use io_locomotion as locomotion;
use io_traversal::navigation::{Domain, Stats};
use io_traversal::{NavigationError, NavigationRequest, NavigationTicket};
use io_types::{Envelope, Vec3};
use io_world::WorldView;

#[derive(Clone, Debug)]
pub(crate) enum TraversalService {
    Surface(locomotion::Movement),
    Athletics(io_traversal::Movement<athletics::Executor>),
}
impl TraversalService {
    pub fn new(
        domain: Domain,
        budget: usize,
        actors: Vec<locomotion::ActorBinding>,
        settings: Option<athletics::Settings>,
        overrides: &[Option<athletics::Settings>],
        planning: io_traversal::PlanningMode,
        world: &dyn WorldView,
    ) -> Result<Self, String> {
        match settings {
            None => Ok(Self::Surface(
                locomotion::Movement::new(domain, budget, actors, world)?.with_planning(planning),
            )),
            Some(settings) => {
                let bindings = actors
                    .into_iter()
                    .zip(overrides)
                    .map(|(a, specific)| {
                        Ok(io_traversal::ActorBinding {
                            executor: athletics::Executor::new(
                                a.motor,
                                specific.unwrap_or(settings),
                                world,
                            )?,
                            familiar_points: a.familiar_points,
                            look_ahead: a.steering.look_ahead,
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                Ok(Self::Athletics(
                    io_traversal::Movement::new(domain, budget, bindings, world)?
                        .with_planning(planning),
                ))
            }
        }
    }
    pub fn request(
        &mut self,
        r: Envelope<NavigationRequest>,
    ) -> Envelope<Result<NavigationTicket, NavigationError>> {
        match self {
            Self::Surface(m) => m.request(r),
            Self::Athletics(m) => m.request(r),
        }
    }
    pub fn actor(&self, id: u64) -> Option<locomotion::ActorFeedback> {
        match self {
            Self::Surface(m) => m.actor(id),
            Self::Athletics(m) => m.actor(id),
        }
    }
    pub fn actors(&self) -> std::vec::IntoIter<locomotion::ActorFeedback> {
        match self {
            Self::Surface(m) => m.actors().collect::<Vec<_>>().into_iter(),
            Self::Athletics(m) => m.actors().collect::<Vec<_>>().into_iter(),
        }
    }
    pub fn stats(&self) -> Stats {
        match self {
            Self::Surface(m) => m.stats(),
            Self::Athletics(m) => m.stats(),
        }
    }
    pub fn planning_stats(&self) -> io_traversal::PlanningStats {
        match self {
            Self::Surface(m) => m.planning_stats(),
            Self::Athletics(m) => m.planning_stats(),
        }
    }
    pub fn route(&self, actor: u64) -> Vec<io_types::Vec3> {
        match self {
            Self::Surface(m) => m
                .route(actor)
                .unwrap_or_default()
                .iter()
                .map(|s| s.position)
                .collect(),
            Self::Athletics(m) => m
                .route(actor)
                .unwrap_or_default()
                .iter()
                .map(|s| s.position)
                .collect(),
        }
    }
    pub fn has_route(&self, actor: u64) -> bool {
        match self {
            Self::Surface(m) => m.route(actor).is_some_and(|r| !r.is_empty()),
            Self::Athletics(m) => m.route(actor).is_some_and(|r| !r.is_empty()),
        }
    }
    pub fn local_trajectory(&self, actor: u64) -> Vec<Vec3> {
        match self {
            Self::Surface(m) => m
                .executor(actor)
                .map_or_else(Vec::new, |e| e.local_trajectory().to_vec()),
            Self::Athletics(_) => vec![],
        }
    }
    pub fn trajectory_stats(&self) -> io_locomotion::trajectory::TrajectoryStats {
        let mut sum = io_locomotion::trajectory::TrajectoryStats::default();
        if let Self::Surface(m) = self {
            for actor in m.actors() {
                if let Some(e) = m.executor(actor.ticket.actor) {
                    let s = e.trajectory_stats();
                    sum.proposals += s.proposals;
                    sum.checks += s.checks;
                    sum.accepted += s.accepted;
                    sum.rejected += s.rejected;
                    sum.completed += s.completed;
                    sum.anticipations += s.anticipations;
                }
            }
        }
        sum
    }
    pub fn validate(&self, w: &dyn WorldView) -> Result<(), Error> {
        match self {
            Self::Surface(m) => m.validate(w),
            Self::Athletics(m) => m.validate(w),
        }
    }
    pub fn before_step<E>(&mut self, w: &mut PluginWorld<E>, dt: f32) -> Result<(), Error> {
        match self {
            Self::Surface(m) => m.before_step(w, dt),
            Self::Athletics(m) => m.before_step(w, dt),
        }
    }
    pub fn update<E>(
        &mut self,
        w: &mut PluginWorld<E>,
        dt: f32,
    ) -> Result<locomotion::MovementFrame, Error> {
        match self {
            Self::Surface(m) => m.update(w, dt),
            Self::Athletics(m) => m.update(w, dt),
        }
    }
}
