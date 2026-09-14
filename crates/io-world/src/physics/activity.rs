use super::{Activity, Body, BodyKind, PhysicsSettings, PhysicsStats};
use io_types::Vec3;

pub(super) fn links(count: usize, edges: &[(usize, usize)]) -> Vec<Vec<usize>> {
    let mut links = vec![Vec::new(); count];
    for &(a, b) in edges {
        links[a].push(b);
        links[b].push(a);
    }
    links
}

pub(super) fn wake_connected(
    seed: usize,
    bodies: &mut [Body],
    links: &[Vec<usize>],
    queue: &mut Vec<usize>,
) -> usize {
    if bodies[seed].kind != BodyKind::Dynamic || bodies[seed].activity != Activity::Sleeping {
        return 0;
    }
    let start = queue.len();
    bodies[seed].activity = Activity::Awake { quiet_seconds: 0. };
    queue.push(seed);
    let mut cursor = start;
    while cursor < queue.len() {
        let id = queue[cursor];
        cursor += 1;
        for &other in &links[id] {
            // Fixed supports do not connect otherwise independent piles.
            if bodies[other].kind == BodyKind::Dynamic
                && bodies[other].activity == Activity::Sleeping
            {
                bodies[other].activity = Activity::Awake { quiet_seconds: 0. };
                queue.push(other);
            }
        }
    }
    queue.len() - start
}

fn root(parents: &mut [usize], mut id: usize) -> usize {
    while parents[id] != id {
        parents[id] = parents[parents[id]];
        id = parents[id];
    }
    id
}

pub(super) fn settle(
    bodies: &mut [Body],
    edges: &[(usize, usize)],
    settings: PhysicsSettings,
    dt: f32,
    stats: &mut PhysicsStats,
) {
    let mut parents: Vec<_> = (0..bodies.len()).collect();
    for &(a, b) in edges {
        if bodies[a].kind == BodyKind::Dynamic && bodies[b].kind == BodyKind::Dynamic {
            let a = root(&mut parents, a);
            let b = root(&mut parents, b);
            parents[a.max(b)] = a.min(b);
        }
    }
    let mut quiet = vec![true; bodies.len()];
    let mut supported = vec![false; bodies.len()];
    let mut gravity_free = vec![true; bodies.len()];
    let mut occupied = vec![false; bodies.len()];
    for (id, body) in bodies.iter_mut().enumerate() {
        if body.kind != BodyKind::Dynamic {
            continue;
        }
        let group = root(&mut parents, id);
        occupied[group] = true;
        gravity_free[group] &= settings.gravity.scaled(body.gravity_scale) == Vec3::default();
        if let Activity::Awake { quiet_seconds } = &mut body.activity {
            let slow = !body.disturbed
                && body.velocity.dot(body.velocity) <= settings.sleep.linear_threshold.powi(2)
                && body.omega.dot(body.omega) <= settings.sleep.angular_threshold.powi(2);
            *quiet_seconds = if slow { *quiet_seconds + dt } else { 0. };
            quiet[group] &= *quiet_seconds >= settings.sleep.idle_seconds;
        }
    }
    for &(a, b) in edges {
        for (dynamic, support) in [(a, b), (b, a)] {
            if bodies[dynamic].kind == BodyKind::Dynamic
                && bodies[support].kind != BodyKind::Dynamic
            {
                let group = root(&mut parents, dynamic);
                supported[group] = true;
                quiet[group] &= !bodies[support].kinematic_moving;
            }
        }
    }
    stats.islands = occupied.iter().filter(|&&v| v).count();
    for (id, body) in bodies.iter_mut().enumerate() {
        if body.kind != BodyKind::Dynamic {
            continue;
        }
        let group = root(&mut parents, id);
        if settings.sleep.enabled && quiet[group] && (supported[group] || gravity_free[group]) {
            body.activity = Activity::Sleeping;
            body.velocity = Vec3::default();
            body.omega = Vec3::default();
        }
        match body.activity {
            Activity::Sleeping => stats.sleeping += 1,
            Activity::Awake { .. } => stats.awake += 1,
        }
    }
}
