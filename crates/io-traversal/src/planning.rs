//! Deferred route search. Only the simulation owns objectives and live execution.
use crate::{
    approach::Approach,
    navigation::{
        Agent, Navigation, RefineProgress, RefineRequest, RouteProvider, RouteRefinement, Stats,
        Status,
    },
    NavigationGoal, NavigationTicket,
};
use io_game::stage::Stage;
use io_types::Vec3;
use io_world::WorldSnapshot;
use serde::Deserialize;
use std::{
    collections::{BTreeMap, VecDeque},
    fmt,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender, TrySendError},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const CAPACITY: usize = 32;
const MAX_EXPANSIONS: u64 = 65_536;

/// Inline is useful for deterministic accelerated tests; Background never waits in a tick.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlanningMode {
    #[default]
    Inline,
    Background,
}

#[derive(Clone, Copy, Debug, Default, serde::Serialize)]
pub struct PlanningStats {
    pub submitted: u64,
    pub completed: u64,
    pub accepted: u64,
    pub rejected: u64,
    pub cancelled: u64,
    pub queue_full: u64,
    pub pending: usize,
    pub last_job_ms: f64,
    pub max_job_ms: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PlanKey {
    pub ticket: NavigationTicket,
    pub attempt: u64,
}

pub(crate) struct PlanJob<P: RouteProvider> {
    pub key: PlanKey,
    pub revision: u64,
    pub start: Vec3,
    pub goal: NavigationGoal,
    pub agent: Agent<P>,
    pub world: Arc<WorldSnapshot>,
    pub familiar: Vec<Vec3>,
    pub horizon: f32,
}
pub(crate) struct PlanResult<P: RouteProvider> {
    pub key: PlanKey,
    pub revision: u64,
    pub start: Vec3,
    pub agent: Agent<P>,
    pub stats: Stats,
    pub elapsed_ms: f64,
}
struct Work<P: RouteProvider> {
    job: PlanJob<P>,
    cancelled: Arc<AtomicBool>,
    approach: Approach,
    started: Instant,
    initialized: bool,
    destinations: usize,
    slices: u64,
    stats: Stats,
    ready: Option<PlanResult<P>>,
    refinement: RouteRefinement<P>,
}
impl<P: RouteProvider> Work<P> {
    fn advance(&mut self, nav: &mut Navigation<P>) -> bool {
        let job = &mut self.job;
        if !self.initialized {
            let destination = match job.goal {
                NavigationGoal::Position(p) => Some(p),
                NavigationGoal::Approach {
                    position, distance, ..
                } => self.approach.destination(
                    nav,
                    &*job.world,
                    &job.agent,
                    job.start,
                    position,
                    distance,
                ),
            };
            self.initialized = true;
            match destination {
                Some(p) => {
                    if job.agent.set_goal(p).is_err() {
                        job.agent.fail();
                        return true;
                    }
                }
                None => {
                    job.agent.unreachable();
                    return true;
                }
            }
        }
        if let Some(p) = job.familiar.pop() {
            nav.familiarize(&*job.world, &mut job.agent, &[p]);
        }
        // Fairness and cancellation are checked between expansions, not whole searches.
        if job.agent.status() == Status::Planning {
            nav.advance(&*job.world, &mut job.agent, job.start, 1);
        }
        self.slices += 1;
        if self.slices >= MAX_EXPANSIONS {
            job.agent.fail();
        }
        match job.agent.status() {
            Status::Planning => false,
            Status::Following => match self.refinement.run(RefineRequest {
                navigation: nav,
                world: &*job.world,
                agent: &mut job.agent,
                start: job.start,
                horizon: job.horizon,
            }) {
                Ok(RefineProgress::Pending) => false,
                Ok(RefineProgress::Complete) => true,
                Err(_) => {
                    job.agent.fail();
                    true
                }
            },
            Status::Unreachable | Status::Blocked if self.destinations < 15 => {
                if let NavigationGoal::Approach {
                    position, distance, ..
                } = job.goal
                {
                    if let Some(p) = self.approach.destination(
                        nav,
                        &*job.world,
                        &job.agent,
                        job.start,
                        position,
                        distance,
                    ) {
                        self.destinations += 1;
                        if job.agent.set_goal(p).is_ok() {
                            return false;
                        }
                        job.agent.fail();
                    }
                }
                true
            }
            _ => true,
        }
    }
}

struct Runtime<P: RouteProvider> {
    send: SyncSender<Work<P>>,
    receive: Mutex<Receiver<PlanResult<P>>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}
impl<P: RouteProvider> Drop for Runtime<P> {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        // Shutdown only, never while polling results. Providers must be bounded/nonblocking.
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
pub(crate) struct Planner<P: RouteProvider> {
    mode: PlanningMode,
    runtime: Option<Runtime<P>>,
    inflight: BTreeMap<u64, (PlanKey, Arc<AtomicBool>)>,
    results: BTreeMap<u64, PlanResult<P>>,
    stats: PlanningStats,
}
impl<P: RouteProvider> Clone for Planner<P> {
    fn clone(&self) -> Self {
        // Published game snapshots never own a live worker. A simulated clone lazily
        // starts an independent worker and resubmits its own missing attempts.
        Self {
            mode: self.mode,
            runtime: None,
            inflight: BTreeMap::new(),
            results: BTreeMap::new(),
            stats: self.stats,
        }
    }
}
impl<P: RouteProvider> fmt::Debug for Planner<P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Planner")
            .field("mode", &self.mode)
            .field("stats", &self.stats)
            .finish()
    }
}
impl<P: RouteProvider> Planner<P> {
    pub fn new(mode: PlanningMode) -> Self {
        Self {
            mode,
            runtime: None,
            inflight: BTreeMap::new(),
            results: BTreeMap::new(),
            stats: PlanningStats::default(),
        }
    }
    pub fn stats(&self) -> PlanningStats {
        self.stats
    }
    pub fn admission_available(&mut self, actor: u64) -> bool {
        let available = self.inflight.len() < CAPACITY || self.inflight.contains_key(&actor);
        if !available {
            self.stats.queue_full += 1;
        }
        available
    }
    pub fn accepted(&mut self) {
        self.stats.accepted += 1;
    }
    pub fn rejected(&mut self) {
        self.stats.rejected += 1;
    }
    pub fn contains(&self, key: PlanKey) -> bool {
        self.inflight
            .get(&key.ticket.actor)
            .is_some_and(|v| v.0 == key)
    }
    pub fn cancel(&mut self, actor: u64) {
        if let Some((_, flag)) = self.inflight.remove(&actor) {
            flag.store(true, Ordering::Release);
            self.stats.cancelled += 1;
        }
        self.results.remove(&actor);
        self.stats.pending = self.inflight.len();
    }
    fn start(&mut self, config: P::Config) -> Result<(), crate::Error> {
        if self.runtime.is_some() {
            return Ok(());
        }
        let nav = Navigation::<P>::new(config).map_err(|_| crate::Error::InvalidInput)?;
        let (send, jobs) = mpsc::sync_channel(CAPACITY);
        let (results, receive) = mpsc::sync_channel(CAPACITY);
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let thread = thread::Builder::new()
            .name("io-planning".into())
            .spawn(move || run(nav, jobs, results, flag))
            .map_err(|_| crate::Error::InvalidWorld)?;
        self.runtime = Some(Runtime {
            send,
            receive: Mutex::new(receive),
            stop,
            thread: Some(thread),
        });
        Ok(())
    }
    pub fn submit(&mut self, job: PlanJob<P>, config: P::Config) -> Result<bool, crate::Error> {
        if self.inflight.len() >= CAPACITY && !self.inflight.contains_key(&job.key.ticket.actor) {
            self.stats.queue_full += 1;
            return Ok(false);
        }
        self.start(config)?;
        self.cancel(job.key.ticket.actor);
        let key = job.key;
        let cancelled = Arc::new(AtomicBool::new(false));
        let work = Work {
            job,
            cancelled: cancelled.clone(),
            approach: Approach::default(),
            started: Instant::now(),
            initialized: false,
            destinations: 0,
            slices: 0,
            stats: Stats::default(),
            ready: None,
            refinement: RouteRefinement::default(),
        };
        match self.runtime.as_ref().unwrap().send.try_send(work) {
            Ok(()) => {
                self.inflight.insert(key.ticket.actor, (key, cancelled));
                self.stats.submitted += 1;
                self.stats.pending = self.inflight.len();
                Ok(true)
            }
            Err(TrySendError::Full(_)) => {
                self.stats.queue_full += 1;
                Ok(false)
            }
            Err(TrySendError::Disconnected(_)) => Err(crate::Error::InvalidWorld),
        }
    }
    pub fn poll(&mut self) {
        let Some(runtime) = &mut self.runtime else {
            return;
        };
        let receive = runtime.receive.get_mut().unwrap_or_else(|e| e.into_inner());
        for result in receive.try_iter().take(CAPACITY) {
            if self
                .inflight
                .get(&result.key.ticket.actor)
                .is_some_and(|v| v.0 == result.key)
            {
                self.stats.completed += 1;
                self.stats.last_job_ms = result.elapsed_ms;
                self.stats.max_job_ms = self.stats.max_job_ms.max(result.elapsed_ms);
                self.results.insert(result.key.ticket.actor, result);
            } else {
                self.stats.rejected += 1;
            }
        }
    }
    pub fn take(&mut self, key: PlanKey) -> Option<PlanResult<P>> {
        if !self
            .results
            .get(&key.ticket.actor)
            .is_some_and(|v| v.key == key)
        {
            return None;
        }
        self.inflight.remove(&key.ticket.actor);
        self.stats.pending = self.inflight.len();
        self.results.remove(&key.ticket.actor)
    }
}

#[cfg(test)]
#[path = "planning_tests.rs"]
mod tests;

fn run<P: RouteProvider>(
    mut nav: Navigation<P>,
    receive: Receiver<Work<P>>,
    send: SyncSender<PlanResult<P>>,
    stop: Arc<AtomicBool>,
) {
    let mut work = VecDeque::<Work<P>>::new();
    while !stop.load(Ordering::Acquire) {
        work.retain(|w| !w.cancelled.load(Ordering::Acquire));
        for w in receive.try_iter().take(CAPACITY) {
            if !w.cancelled.load(Ordering::Acquire) {
                work.push_back(w);
            }
        }
        let Some(mut next) = work.pop_front() else {
            match receive.recv_timeout(Duration::from_millis(2)) {
                Ok(w) => work.push_back(w),
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            continue;
        };
        if next.cancelled.load(Ordering::Acquire) {
            continue;
        }
        if next.ready.is_none() {
            let before = nav.stats();
            let done =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| next.advance(&mut nav)))
                    .unwrap_or_else(|_| {
                        next.job.agent.fail();
                        true
                    });
            let after = nav.stats();
            next.stats.expanded += after.expanded - before.expanded;
            next.stats.clearance_queries += after.clearance_queries - before.clearance_queries;
            next.stats.cache_hits += after.cache_hits - before.cache_hits;
            next.stats.invalidations += after.invalidations - before.invalidations;
            next.stats.steering_queries += after.steering_queries - before.steering_queries;
            next.stats.refinement_queries += after.refinement_queries - before.refinement_queries;
            if done {
                next.ready = Some(PlanResult {
                    key: next.job.key,
                    revision: next.job.revision,
                    start: next.job.start,
                    agent: next.job.agent.clone(),
                    stats: next.stats,
                    elapsed_ms: next.started.elapsed().as_secs_f64() * 1000.,
                });
            }
        }
        if next.cancelled.load(Ordering::Acquire) {
            continue;
        }
        if let Some(result) = next.ready.take() {
            match send.try_send(result) {
                Ok(()) => continue,
                Err(TrySendError::Disconnected(_)) => break,
                Err(TrySendError::Full(result)) => {
                    next.ready = Some(result);
                    thread::sleep(Duration::from_millis(1));
                }
            }
        }
        work.push_back(next);
    }
}
