//! Snapshot-only route preparation. The existing waypoint/action contract is retained.
use super::{distance, Agent, Navigation, RouteProvider, State, Status, Waypoint};
use io_game::stage::Stage;
use io_types::Vec3;
use io_world::WorldView;
use std::sync::Arc;

pub struct RefineRequest<'a, P: RouteProvider> {
    pub navigation: &'a mut Navigation<P>,
    pub world: &'a dyn WorldView,
    pub agent: &'a mut Agent<P>,
    pub start: Vec3,
    pub horizon: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefineProgress {
    Pending,
    Complete,
}

/// Ticket/attempt ownership is checked by the coordinator before this stage.
/// This stage checks capability/revision compatibility and the live join connector.
pub struct HandoffRequest<'a, P: RouteProvider> {
    pub navigation: &'a mut Navigation<P>,
    pub world: &'a dyn WorldView,
    pub agent: &'a mut Agent<P>,
    pub planned_start: Vec3,
    pub position: Vec3,
    pub horizon: f32,
    pub snapshot_revision: u64,
    pub current_profile: P::Profile,
}

pub struct RouteHandoff;
impl<P: RouteProvider> Stage<HandoffRequest<'_, P>> for RouteHandoff {
    type Output = bool;
    type Error = &'static str;
    fn run(&mut self, input: HandoffRequest<'_, P>) -> Result<bool, Self::Error> {
        let HandoffRequest {
            navigation: nav,
            world,
            agent,
            planned_start,
            position,
            horizon,
            snapshot_revision,
            current_profile,
        } = input;
        if !planned_start.finite() || !position.finite() || !horizon.is_finite() || horizon <= 0. {
            return Err("invalid handoff input");
        }
        if agent.profile() != current_profile
            || (snapshot_revision != nav.revision(world)
                && !(nav.live_checked() && agent.status() == Status::Following))
        {
            return Ok(false);
        }
        Ok(nav.accept_handoff(world, agent, planned_start, position, horizon))
    }
}

#[derive(Clone, Debug)]
struct Cursor<P: RouteProvider> {
    source: Arc<[Waypoint<P::Action>]>,
    profile: P::Profile,
    revision: u64,
    actor: Option<u64>,
    horizon: f32,
    start: Vec3,
    route_next: usize,
    next: usize,
    origin: Vec3,
    candidate: Option<usize>,
    attempts: usize,
    output: Vec<Waypoint<P::Action>>,
    phase: RefinementPhase,
}

#[derive(Clone, Debug)]
enum RefinementPhase {
    Shortcuts,
    Merge(usize),
    Complete,
}

/// One candidate test per invocation, allowing the planner's existing round-robin
/// queue to bound work and check cancellation between expensive geometry calls.
/// Keep the input world/start/profile stable until Complete. No live world is mutated.
#[derive(Clone, Debug)]
pub struct RouteRefinement<P: RouteProvider> {
    cursor: Option<Cursor<P>>,
}
impl<P: RouteProvider> Default for RouteRefinement<P> {
    fn default() -> Self {
        Self { cursor: None }
    }
}
impl<P: RouteProvider> Stage<RefineRequest<'_, P>> for RouteRefinement<P> {
    type Output = RefineProgress;
    type Error = &'static str;

    fn run(&mut self, input: RefineRequest<'_, P>) -> Result<Self::Output, Self::Error> {
        let RefineRequest {
            navigation: nav,
            world,
            agent,
            start,
            horizon,
        } = input;
        if !start.finite() || !horizon.is_finite() || horizon <= 0. {
            return Err("invalid refinement input");
        }
        let revision = nav.revision(world);
        let State::Route {
            steps,
            next,
            refined,
        } = &mut agent.state
        else {
            self.cursor = None;
            return Ok(RefineProgress::Complete);
        };
        if *refined {
            self.cursor = None;
            return Ok(RefineProgress::Complete);
        }
        if agent.revision != Some(revision) {
            return Err("refinement snapshot does not match route");
        }
        if let Some(c) = &self.cursor {
            if !Arc::ptr_eq(&c.source, steps)
                || c.route_next != *next
                || c.profile != agent.profile
                || c.actor != agent.actor
                || c.revision != revision
                || c.start != start
                || c.horizon != horizon
            {
                return Err("refinement inputs changed while pending");
            }
        } else {
            self.cursor = Some(Cursor {
                source: steps.clone(),
                profile: agent.profile,
                actor: agent.actor,
                revision,
                horizon,
                start,
                route_next: *next,
                next: *next,
                origin: start,
                candidate: None,
                attempts: 0,
                output: Vec::new(),
                phase: RefinementPhase::Shortcuts,
            });
        }
        let c = self.cursor.as_mut().unwrap();
        match &mut c.phase {
            RefinementPhase::Shortcuts => {
                if c.next < c.source.len() {
                    let first = c.source[c.next];
                    let candidate = *c.candidate.get_or_insert_with(|| {
                        if !nav.provider.automatic_completion(first.action) {
                            return c.next;
                        }
                        c.source
                            .iter()
                            .enumerate()
                            .skip(c.next)
                            .take(32)
                            .take_while(|(_, p)| {
                                p.action == first.action
                                    && distance(c.origin, p.position) <= horizon
                            })
                            .last()
                            .map_or(c.next, |(i, _)| i)
                    });
                    let accepted = if candidate == c.next {
                        // This is the original planned edge, not a new shortcut. In
                        // particular, never replace a discrete action with a point ray.
                        true
                    } else {
                        nav.stats.refinement_queries += 1;
                        nav.provider.segment_clear(
                            world,
                            agent.profile,
                            agent.actor,
                            c.origin,
                            c.source[candidate].position,
                            first.action,
                        )
                    };
                    if accepted {
                        let step = c.source[candidate];
                        c.output.push(step);
                        c.origin = step.position;
                        c.next = candidate + 1;
                        c.candidate = None;
                        c.attempts = 0;
                    } else {
                        c.attempts += 1;
                        c.candidate = Some(if c.attempts >= 7 {
                            c.next
                        } else {
                            candidate - 1
                        });
                    }
                }
                if c.next == c.source.len() {
                    c.phase = RefinementPhase::Merge(0);
                }
            }
            RefinementPhase::Merge(index) => {
                if *index + 2 < c.output.len() {
                    let i = *index;
                    let a = c.output[i];
                    let b = c.output[i + 1];
                    let ab = b.position - a.position;
                    let len = ab.dot(ab);
                    let mut end = i + 1;
                    if nav.provider.automatic_completion(a.action)
                        && a.action == b.action
                        && len > 1e-12
                    {
                        for j in (i + 2)..c.output.len().min(i + 33) {
                            let step = c.output[j];
                            let delta = step.position - c.output[j - 1].position;
                            let error = delta - ab.scaled(delta.dot(ab) / len);
                            if step.action != a.action
                                || ab.dot(delta) <= 0.
                                || error.dot(error) > 1e-10
                            {
                                break;
                            }
                            end = j;
                        }
                    }
                    if end > i + 1 {
                        nav.stats.refinement_queries += 1;
                        // Check a whole straight run once, not increasingly long prefixes.
                        // Providers can reject a merged connection's length or geometry.
                        if nav.provider.segment_clear(
                            world,
                            agent.profile,
                            agent.actor,
                            a.position,
                            c.output[end].position,
                            a.action,
                        ) {
                            c.output.drain((i + 1)..end);
                        }
                    }
                    *index += 1;
                } else {
                    c.phase = RefinementPhase::Complete;
                }
            }
            RefinementPhase::Complete => {}
        }
        if matches!(c.phase, RefinementPhase::Complete) {
            *steps = std::mem::take(&mut c.output).into();
            *next = 0;
            *refined = true;
            self.cursor = None;
            Ok(RefineProgress::Complete)
        } else {
            Ok(RefineProgress::Pending)
        }
    }
}

impl<P: RouteProvider> Navigation<P> {
    /// Explicit synchronous preparation for inline/deterministic callers. Background
    /// callers run RouteRefinement in slices on their worker instead.
    pub fn refine_route(
        &mut self,
        world: &dyn WorldView,
        agent: &mut Agent<P>,
        start: Vec3,
        horizon: f32,
    ) -> Result<(), &'static str> {
        let mut stage = RouteRefinement::default();
        while stage.run(RefineRequest {
            navigation: self,
            world,
            agent,
            start,
            horizon,
        })? == RefineProgress::Pending
        {}
        Ok(())
    }

    /// Select already-prepared waypoints without running shortcut geometry queries.
    /// Only collinear continuation can be skipped arithmetically; turns still use
    /// advance's live near-corner connector check and the executor's immediate checks.
    pub fn prepared_target(
        &self,
        agent: &mut Agent<P>,
        position: Vec3,
        horizon: f32,
    ) -> Option<Waypoint<P::Action>> {
        let State::Route {
            steps,
            next,
            refined,
        } = &mut agent.state
        else {
            return None;
        };
        if !position.finite() || !horizon.is_finite() || horizon <= 0. {
            return None;
        }
        if !*refined {
            return steps.get(*next).copied();
        }
        for _ in 0..32 {
            let first = *steps.get(*next)?;
            let Some(second) = steps.get(*next + 1) else {
                break;
            };
            if !self.provider.automatic_completion(first.action)
                || first.action != second.action
                || distance(position, first.position) > horizon
            {
                break;
            }
            let a = first.position - position;
            let b = second.position - first.position;
            let length = b.dot(b);
            // Do not cross action boundaries, reversals, slopes or a corner by
            // assuming proximity is clearance. The two segments must lie on one ray.
            if length <= 1e-12 || a.dot(b) < 0. {
                break;
            }
            let error = a - b.scaled(a.dot(b) / length);
            if error.dot(error) > 1e-12 {
                break;
            }
            *next += 1;
        }
        steps.get(*next).copied()
    }
}

#[cfg(test)]
#[path = "refinement_tests.rs"]
mod tests;
