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
