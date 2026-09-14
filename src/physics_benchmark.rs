use crate::contact_diagnostics_report::{Builder, ContactReport};
use std::path::Path;

#[derive(serde::Serialize)]
pub struct PhysicsBenchmark {
    pub tick_hz: u32,
    pub simulated_seconds: f64,
    pub solver_mode: &'static str,
    pub substeps: u32,
    pub iterations: u32,
    pub total_solver_visits: usize,
    pub total_broad_phase_builds: usize,
    pub total_narrow_phase_calls: usize,
    pub total_reused_contact_points: usize,
    pub total_refreshed_manifolds: usize,
    pub total_bound_escape_rebuilds: usize,
    pub peak_anchor_bytes: usize,
    pub audited_final_penetration: f32,
    pub audited_final_overlapping_pairs: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contact_diagnostics: Option<ContactReport>,
    pub frames: usize,
    pub bodies: usize,
    pub moving_bodies: usize,
    pub simulation_mean_ms: f64,
    pub simulation_p95_ms: f64,
    pub solver_mean_ms: f64,
    pub solver_p95_ms: f64,
    pub peak_candidate_pairs: usize,
    pub peak_contact_points: usize,
    pub total_contact_events: usize,
    pub max_displacement: f32,
    pub final_awake_bodies: usize,
    pub final_sleeping_bodies: usize,
    pub final_islands: usize,
    pub peak_pair_checks: usize,
    pub final_pair_checks: usize,
    pub total_integrations: usize,
    pub propagated_wakes: usize,
    pub tail_simulation_mean_ms: f64,
    pub collision_mean_ms: f64,
    pub resolution_mean_ms: f64,
    pub warm_start_hits: usize,
    pub warm_start_misses: usize,
    pub peak_cached_contacts: usize,
    pub warm_start_mean_ms: f64,
    pub peak_contact_penetration: f32,
    pub final_contact_penetration: f32,
    pub preparation_mean_ms: f64,
    pub iteration_mean_ms: f64,
    pub contact_record_bytes: usize,
    pub prepared_record_bytes: usize,
    pub peak_contact_working_bytes: usize,
    pub prepared_capacity_bytes: usize,
}
pub fn benchmark_physics(path: &Path, frames: usize) -> Result<PhysicsBenchmark, String> {
    benchmark_physics_with_diagnostics(path, frames, false)
}
pub fn benchmark_physics_with_diagnostics(
    path: &Path,
    frames: usize,
    diagnostics: bool,
) -> Result<PhysicsBenchmark, String> {
    if !(1..=36000).contains(&frames) {
        return Err("frames must be within 1..36000".into());
    }
    let library = crate::model::ModelLibrary::load(path)?;
    let timing = library.config.simulation;
    let mut world = crate::demo::world(&library)?;
    world.set_contact_diagnostics(diagnostics);
    let mut diagnostic_report = diagnostics.then(Builder::default);
    if world.physics_indices().is_empty() {
        return Err("scene has no physics bodies".into());
    }
    let initial: Vec<_> = world.items().iter().map(|i| i.transform.anchor).collect();
    let mut times = Vec::with_capacity(frames);
    let mut solver = Vec::with_capacity(frames);
    let mut candidates = 0;
    let mut contacts = 0;
    let mut events = 0;
    let mut checks = 0;
    let mut integrations = 0;
    let mut wakes = 0;
    let mut collision = 0.;
    let mut resolution = 0.;
    let mut warm_hits = 0;
    let mut warm_misses = 0;
    let mut cached = 0;
    let mut warm_seconds = 0.;
    let mut penetration = 0_f32;
    let mut preparation = 0.;
    let mut iteration = 0.;
    let mut contact_bytes = 0;
    let mut visits = 0;
    let mut builds = 0;
    let mut narrow_calls = 0;
    let mut reused = 0;
    let mut refreshed = 0;
    let mut escaped = 0;
    let mut anchors = 0;
    for tick in 1..=frames {
        let start = std::time::Instant::now();
        world.simulate(&[], timing.seconds() as f32);
        times.push(start.elapsed().as_secs_f64() * 1000.);
        if let Some(error) = world.physics_error() {
            return Err(error.into());
        }
        let stats = world.physics_stats();
        visits += stats.solver_visits;
        builds += stats.broad_phase_builds;
        narrow_calls += stats.narrow_phase_calls;
        reused += stats.reused_contact_points;
        refreshed += stats.refreshed_manifolds;
        escaped += stats.bound_escape_rebuilds;
        anchors = anchors.max(stats.peak_anchor_bytes);
        if let (Some(report), Some(data)) = (&mut diagnostic_report, world.contact_diagnostics()) {
            report.record(tick, &data.passes);
        }
        solver.push(stats.step_seconds * 1000.);
        candidates = candidates.max(stats.candidate_pairs);
        contacts = contacts.max(stats.contacts);
        events += world.contacts().len();
        checks = checks.max(stats.pair_checks);
        integrations += stats.integrated;
        wakes += stats.woken;
        collision += stats.collision_seconds;
        resolution += stats.resolution_seconds;
        warm_hits += stats.warm_start_hits;
        warm_misses += stats.warm_start_misses;
        cached = cached.max(stats.peak_cached_contacts);
        warm_seconds += stats.warm_start_seconds;
        penetration = penetration.max(stats.max_penetration);
        preparation += stats.preparation_seconds;
        iteration += stats.iteration_seconds;
        contact_bytes = contact_bytes.max(stats.peak_contact_working_bytes);
    }
    let stats = world.physics_stats();
    let settings = world.physics_settings();
    let audit = world.physics_overlap_audit();
    let mut maximum = 0_f32;
    for (item, initial) in world.items().iter().zip(initial) {
        let delta = item.transform.anchor - initial;
        maximum = maximum.max(delta.dot(delta).sqrt());
    }
    let mean = times.iter().sum::<f64>() / frames as f64;
    let tail = &times[frames.saturating_sub(30)..];
    let tail_mean = tail.iter().sum::<f64>() / tail.len() as f64;
    let solver_mean = solver.iter().sum::<f64>() / frames as f64;
    times.sort_by(f64::total_cmp);
    solver.sort_by(f64::total_cmp);
    let p95 = (frames as f64 * 0.95).ceil() as usize - 1;
    Ok(PhysicsBenchmark {
        tick_hz: timing.tick_hz(),
        simulated_seconds: frames as f64 * timing.seconds(),
        solver_mode: match settings.solver {
            io_world::SolverMode::Pgs => "pgs",
            io_world::SolverMode::Tgs => "tgs",
        },
        substeps: settings.substeps,
        iterations: settings.iterations,
        total_solver_visits: visits,
        total_broad_phase_builds: builds,
        total_narrow_phase_calls: narrow_calls,
        total_reused_contact_points: reused,
        total_refreshed_manifolds: refreshed,
        total_bound_escape_rebuilds: escaped,
        peak_anchor_bytes: anchors,
        audited_final_penetration: audit.max_penetration,
        audited_final_overlapping_pairs: audit.overlapping_pairs,
        contact_diagnostics: diagnostic_report.map(Builder::finish),
        frames,
        bodies: stats.bodies,
        moving_bodies: stats.moving,
        simulation_mean_ms: mean,
        simulation_p95_ms: times[p95],
        solver_mean_ms: solver_mean,
        solver_p95_ms: solver[p95],
        peak_candidate_pairs: candidates,
        peak_contact_points: contacts,
        total_contact_events: events,
        max_displacement: maximum,
        final_awake_bodies: stats.awake,
        final_sleeping_bodies: stats.sleeping,
        final_islands: stats.islands,
        peak_pair_checks: checks,
        final_pair_checks: stats.pair_checks,
        total_integrations: integrations,
        propagated_wakes: wakes,
        tail_simulation_mean_ms: tail_mean,
        collision_mean_ms: collision * 1000. / frames as f64,
        resolution_mean_ms: resolution * 1000. / frames as f64,
        warm_start_hits: warm_hits,
        warm_start_misses: warm_misses,
        peak_cached_contacts: cached,
        warm_start_mean_ms: warm_seconds * 1000. / frames as f64,
        peak_contact_penetration: penetration,
        final_contact_penetration: stats.max_penetration,
        preparation_mean_ms: preparation * 1000. / frames as f64,
        iteration_mean_ms: iteration * 1000. / frames as f64,
        contact_record_bytes: stats.contact_record_bytes,
        prepared_record_bytes: stats.prepared_record_bytes,
        peak_contact_working_bytes: contact_bytes,
        prepared_capacity_bytes: stats.prepared_capacity_bytes,
    })
}
