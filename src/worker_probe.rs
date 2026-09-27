//! Paced CPU-only snapshot/frame-preparation probe. This does not measure GPU FPS.
use crate::{
    camera::Camera,
    model::ModelLibrary,
    projection::Frame,
    worker::{Region, Status, Worker},
};
use io_types::Vec3;
use io_world::WorldView;
use std::{
    path::Path,
    time::{Duration, Instant},
};

#[derive(serde::Serialize)]
pub struct WorkerProbe {
    pub movers: Vec<TravelSample>,
    pub navigation_revisions: u64,
    pub planning: Option<io_traversal::PlanningStats>,
    pub navigation: Option<io_traversal::navigation::Stats>,
    pub discovery: Option<io_playground::DiscoveryStats>,
    pub observed_ticks: Vec<TickSample>,
    pub actors: Vec<ActorSample>,
    pub tick_hz: u32,
    pub frames: usize,
    pub world_items: usize,
    pub wall_seconds: f64,
    pub completed_tick: u64,
    pub simulated_seconds: f64,
    pub overruns: u64,
    pub poll_mean_ms: f64,
    pub poll_p95_ms: f64,
    pub prepare_mean_ms: f64,
    pub prepare_p95_ms: f64,
    pub last_simulation_ms: f64,
    pub last_snapshot_ms: f64,
    pub max_snapshot_age_ms: f64,
    pub min_visible: usize,
    pub max_visible: usize,
    pub cpu_frames_over_16_67_ms: usize,
}

#[derive(serde::Serialize)]
pub struct TickSample {
    wall_ms: f64,
    tick: u64,
    simulation_ms: f64,
    snapshot_ms: f64,
    planning: usize,
    following: usize,
    pending_jobs: usize,
    moving: usize,
    stationary_by_status: std::collections::BTreeMap<String, usize>,
    navigation_revision: u64,
    planning_stats: Option<io_traversal::PlanningStats>,
    navigation_stats: Option<io_traversal::navigation::Stats>,
}
#[derive(serde::Serialize)]
pub struct ActorSample {
    id: u64,
    status: String,
    position: [f32; 3],
    travel: TravelSample,
    activity: ActivitySample,
    goal: Option<[f32; 3]>,
    remaining_route: Vec<[f32; 3]>,
}

#[derive(Clone, Default, serde::Serialize)]
struct StateTime {
    moving_seconds: f64,
    stationary_seconds: f64,
}
/// Sampled motion, not a promise inferred from NavigationStatus::Following.
#[derive(Clone, serde::Serialize)]
struct ActivitySample {
    seconds_by_status: std::collections::BTreeMap<String, StateTime>,
    longest_stationary_seconds: f64,
    current_stationary_seconds: f64,
    #[serde(skip)]
    previous: Vec3,
    #[serde(skip)]
    tick: u64,
}
impl ActivitySample {
    fn new(previous: Vec3) -> Self {
        Self {
            seconds_by_status: Default::default(),
            longest_stationary_seconds: 0.,
            current_stationary_seconds: 0.,
            previous,
            tick: 0,
        }
    }
    fn observe(&mut self, p: Vec3, tick: u64, tick_seconds: f64, status: &str) -> bool {
        let seconds = tick.saturating_sub(self.tick) as f64 * tick_seconds;
        if seconds <= 0. {
            return false;
        }
        let delta = p - self.previous;
        // Below 5cm/s is stationary for this traffic diagnostic. Tiny collision
        // jitter must not turn a stalled actor into an apparently active one.
        let moving = f64::from(delta.dot(delta).sqrt()) / seconds >= 0.05;
        let state = self.seconds_by_status.entry(status.to_owned()).or_default();
        if moving {
            state.moving_seconds += seconds;
            self.current_stationary_seconds = 0.;
        } else {
            state.stationary_seconds += seconds;
            self.current_stationary_seconds += seconds;
            self.longest_stationary_seconds = self
                .longest_stationary_seconds
                .max(self.current_stationary_seconds);
        }
        self.previous = p;
        self.tick = tick;
        moving
    }
}

#[derive(Clone, serde::Serialize)]
pub struct TravelSample {
    id: u64,
    distance: f32,
    max_displacement: f32,
    first_motion_ms: Option<f64>,
    #[serde(skip)]
    start: Vec3,
    #[serde(skip)]
    previous: Vec3,
}
impl TravelSample {
    fn new(id: u64, p: Vec3) -> Self {
        Self {
            id,
            distance: 0.,
            max_displacement: 0.,
            first_motion_ms: None,
            start: p,
            previous: p,
        }
    }
    fn observe(&mut self, p: Vec3, wall_ms: f64) {
        let delta = p - self.previous;
        let length = delta.dot(delta).sqrt();
        if length > 0.0001 {
            self.distance += length;
            self.first_motion_ms.get_or_insert(wall_ms);
        }
        let from_start = p - self.start;
        self.max_displacement = self.max_displacement.max(from_start.dot(from_start).sqrt());
        self.previous = p;
    }
}

fn timings(mut values: Vec<f64>) -> (f64, f64) {
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    values.sort_by(f64::total_cmp);
    (
        mean,
        values[(values.len() as f64 * 0.95).ceil() as usize - 1],
    )
}

pub fn probe_worker(path: &Path, frames: usize) -> Result<WorkerProbe, String> {
    if !(1..=36000).contains(&frames) {
        return Err("frames must be 1..36000".into());
    }
    let library = ModelLibrary::load(path)?;
    let world = crate::demo::world(&library)?;
    let c = &library.config.camera;
    let o = library.config.origin;
    let target = Vec3::new(o[0] + c.target[0], o[1] + c.target[1], o[2] + c.target[2]);
    let mut camera = Camera::new(target);
    camera.set_zoom(c.zoom);
    camera.set_distance(c.render_distance);
    camera.set_viewport(1280, 800);
    let timing = library.config.simulation;
    let game = crate::demo::game(&library, &world)?;
    let initial_navigation_revision = world.navigation_revision();
    let mut travel = std::collections::BTreeMap::new();
    let mut activity = std::collections::BTreeMap::new();
    if let Some(game) = game.traversal() {
        for (id, _, _) in game.navigation_status() {
            activity.insert(
                id,
                ActivitySample::new(world.item(id).unwrap().transform.anchor),
            );
            travel.insert(
                id,
                TravelSample::new(id, world.item(id).unwrap().transform.anchor),
            );
        }
    }
    let mut movers: Vec<_> = world
        .items()
        .iter()
        .filter(|i| {
            i.character_body.is_none()
                && i.physics_body
                    .as_ref()
                    .is_some_and(|b| b.kind == io_world::BodyKind::Kinematic)
        })
        .map(|i| TravelSample::new(i.id, i.transform.anchor))
        .collect();
    let mut worker = Worker::spawn_with_game(
        world,
        vec![Region {
            center: target,
            radius: camera.render_distance(),
            follow: None,
        }],
        timing,
        game,
    )?;
    let mut frame = Frame::default();
    let mut poll = Vec::with_capacity(frames);
    let mut prepare = Vec::with_capacity(frames);
    let mut max_age = 0_f64;
    let mut min_visible = usize::MAX;
    let mut max_visible = 0;
    let mut over_budget = 0;
    let started = Instant::now();
    let mut observed_ticks = Vec::new();
    let period = Duration::from_secs_f64(1. / 60.);
    let mut deadline = started;
    for serial in 1..=frames {
        let begin = Instant::now();
        worker.poll_snapshot();
        if worker.status() != Status::Running {
            return Err(format!("worker stopped: {:?}", worker.status()));
        }
        poll.push(begin.elapsed().as_secs_f64() * 1000.);
        let snapshot = &worker.current;
        if observed_ticks
            .last()
            .is_none_or(|s: &TickSample| s.tick != snapshot.tick)
        {
            let wall_ms = started.elapsed().as_secs_f64() * 1000.;
            for sample in travel.values_mut().chain(movers.iter_mut()) {
                if let Some(item) = snapshot.world.item(sample.id) {
                    sample.observe(item.transform.anchor, wall_ms);
                }
            }
            let statuses = snapshot
                .game
                .traversal()
                .map(|t| t.navigation_status())
                .unwrap_or_default();
            let mut moving = 0;
            let mut stationary_by_status = std::collections::BTreeMap::new();
            for (id, status, _) in &statuses {
                let label = format!("{status:?}");
                if activity.get_mut(id).unwrap().observe(
                    snapshot.world.item(*id).unwrap().transform.anchor,
                    snapshot.tick,
                    timing.seconds(),
                    &label,
                ) {
                    moving += 1;
                } else {
                    *stationary_by_status.entry(label).or_default() += 1;
                }
            }
            observed_ticks.push(TickSample {
                moving,
                stationary_by_status,
                navigation_revision: snapshot.world.navigation_revision(),
                planning_stats: snapshot.game.traversal().and_then(|t| t.planning_stats()),
                navigation_stats: snapshot.game.traversal().and_then(|t| t.navigation_stats()),
                wall_ms: started.elapsed().as_secs_f64() * 1000.,
                tick: snapshot.tick,
                simulation_ms: snapshot.simulation_ms,
                snapshot_ms: snapshot.snapshot_ms,
                planning: statuses
                    .iter()
                    .filter(|s| s.1 == io_traversal::NavigationStatus::Planning)
                    .count(),
                following: statuses
                    .iter()
                    .filter(|s| s.1 == io_traversal::NavigationStatus::Following)
                    .count(),
                pending_jobs: snapshot
                    .game
                    .traversal()
                    .and_then(|t| t.planning_stats())
                    .map_or(0, |s| s.pending),
            });
        }
        max_age = max_age.max(snapshot.completed_at.elapsed().as_secs_f64() * 1000.);
        let building = Instant::now();
        frame.build(
            &snapshot.world,
            &camera,
            serial as u64,
            &library,
            worker.alpha(Instant::now()),
            &snapshot.active,
            false,
        )?;
        prepare.push(building.elapsed().as_secs_f64() * 1000.);
        min_visible = min_visible.min(frame.instances.len());
        max_visible = max_visible.max(frame.instances.len());
        over_budget += usize::from(begin.elapsed() > period);
        deadline += period;
        if let Some(wait) = deadline.checked_duration_since(Instant::now()) {
            std::thread::sleep(wait);
        } else {
            deadline = Instant::now();
        }
    }
    let (poll_mean_ms, poll_p95_ms) = timings(poll);
    let (prepare_mean_ms, prepare_p95_ms) = timings(prepare);
    let snapshot = &worker.current;
    Ok(WorkerProbe {
        movers,
        navigation_revisions: snapshot
            .world
            .navigation_revision()
            .wrapping_sub(initial_navigation_revision),
        planning: snapshot.game.traversal().and_then(|t| t.planning_stats()),
        navigation: snapshot.game.traversal().and_then(|t| t.navigation_stats()),
        discovery: snapshot.game.traversal().and_then(|t| t.discovery_stats()),
        actors: snapshot
            .game
            .traversal()
            .map(|t| {
                t.diagnostics()
                    .into_iter()
                    .filter_map(|d| {
                        let id = d.actor;
                        let p = snapshot.world.item(id)?.transform.anchor;
                        Some(ActorSample {
                            id,
                            status: format!("{:?}", d.feedback.status),
                            position: [p.x, p.y, p.z],
                            travel: travel[&id].clone(),
                            activity: activity[&id].clone(),
                            goal: d.goal.map(|g| match g {
                                io_traversal::NavigationGoal::Position(p)
                                | io_traversal::NavigationGoal::Approach { position: p, .. } => {
                                    [p.x, p.y, p.z]
                                }
                            }),
                            remaining_route: d.route.iter().map(|p| [p.x, p.y, p.z]).collect(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        observed_ticks,
        tick_hz: timing.tick_hz(),
        frames,
        world_items: snapshot.world.items().len(),
        wall_seconds: started.elapsed().as_secs_f64(),
        completed_tick: snapshot.tick,
        simulated_seconds: snapshot.tick as f64 * timing.seconds(),
        overruns: snapshot.overruns,
        poll_mean_ms,
        poll_p95_ms,
        prepare_mean_ms,
        prepare_p95_ms,
        last_simulation_ms: snapshot.simulation_ms,
        last_snapshot_ms: snapshot.snapshot_ms,
        max_snapshot_age_ms: max_age,
        min_visible,
        max_visible,
        cpu_frames_over_16_67_ms: over_budget,
    })
}

#[cfg(test)]
mod activity_tests {
    use super::*;
    #[test]
    fn following_is_not_motion_and_duplicate_snapshots_do_not_add_time() {
        let mut a = ActivitySample::new(Vec3::default());
        assert!(!a.observe(Vec3::default(), 60, 1. / 60., "Following"));
        assert!(!a.observe(Vec3::default(), 60, 1. / 60., "Following"));
        assert_eq!(a.seconds_by_status["Following"].stationary_seconds, 1.);
        assert!(!a.observe(Vec3::new(0.001, 0., 0.), 120, 1. / 60., "Blocked"));
        assert_eq!(a.longest_stationary_seconds, 2.);
        assert!(a.observe(Vec3::new(1., 0., 0.), 180, 1. / 60., "Following"));
        assert_eq!(a.current_stationary_seconds, 0.);
        assert_eq!(a.seconds_by_status["Following"].moving_seconds, 1.);
    }
}
