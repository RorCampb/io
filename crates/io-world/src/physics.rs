//! Limited in-house rigid-body solver. No mesh, camera, or renderer dependency.
mod activity;
mod broad_phase;
mod collision;
mod diagnostics;
mod solver;
mod tgs;
mod warm_start;
use crate::Item;
pub use diagnostics::{
    ContactDiagnostics, ContactPassStats, CORRECTION_SPEED_BOUNDS, CORRECTION_SPEED_THRESHOLD,
};
use io_types::{Bounds, Rotation, Vec3};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BodyKind {
    Static,
    Dynamic,
    Kinematic,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ColliderShape {
    Box { half_extents: Vec3 },
    Sphere { radius: f32 },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Collider {
    pub shape: ColliderShape,
    /// Physical local units, independent of render scale. Also the center of mass.
    pub offset: Vec3,
    pub friction: f32,
    pub restitution: f32,
    pub memberships: u32,
    pub filter: u32,
}
impl Collider {
    pub fn new(shape: ColliderShape) -> Self {
        Self {
            shape,
            offset: Vec3::default(),
            friction: 0.6,
            restitution: 0.,
            memberships: 1,
            filter: u32::MAX,
        }
    }
    pub fn validate(&self) -> Result<(), String> {
        let dimensions = match self.shape {
            ColliderShape::Box { half_extents } => [half_extents.x, half_extents.y, half_extents.z],
            ColliderShape::Sphere { radius } => [radius; 3],
        };
        if dimensions
            .iter()
            .any(|v| !v.is_finite() || !(0.01..=10000.).contains(v))
            || !bounded(self.offset, 10000.)
            || !self.friction.is_finite()
            || !(0.0..=2.).contains(&self.friction)
            || !self.restitution.is_finite()
            || !(0.0..=1.).contains(&self.restitution)
        {
            return Err("invalid collider dimensions, offset, friction, or restitution".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PhysicsBody {
    pub kind: BodyKind,
    pub mass: f32,
    pub gravity_scale: f32,
    pub linear_damping: f32,
    pub angular_damping: f32,
    pub velocity: Vec3,
    pub angular_velocity: Vec3,
    pub(crate) target: Option<(Vec3, Rotation)>,
    activity: Activity,
}
#[derive(Clone, Copy, Debug, PartialEq)]
enum Activity {
    Awake { quiet_seconds: f32 },
    Sleeping,
}
impl PhysicsBody {
    pub fn is_sleeping(&self) -> bool {
        self.activity == Activity::Sleeping
    }
    pub(crate) fn wake(&mut self) {
        self.activity = Activity::Awake { quiet_seconds: 0. };
    }
    pub fn new(kind: BodyKind) -> Self {
        Self {
            kind,
            mass: 1.,
            gravity_scale: 1.,
            linear_damping: 0.,
            angular_damping: 0.05,
            velocity: Vec3::default(),
            angular_velocity: Vec3::default(),
            target: None,
            activity: Activity::Awake { quiet_seconds: 0. },
        }
    }
    pub fn validate(&self) -> Result<(), String> {
        if !self.mass.is_finite()
            || !(0.01..=1e6).contains(&self.mass)
            || !self.gravity_scale.is_finite()
            || !(-10.0..=10.).contains(&self.gravity_scale)
            || [self.linear_damping, self.angular_damping]
                .iter()
                .any(|v| !v.is_finite() || !(0.0..=100.).contains(v))
            || !bounded(self.velocity, 10000.)
            || !bounded(self.angular_velocity, 1000.)
        {
            return Err("invalid body mass, gravity multiplier, damping, or velocity".into());
        }
        if self.kind == BodyKind::Static
            && (self.velocity != Vec3::default() || self.angular_velocity != Vec3::default())
        {
            return Err("static bodies cannot have velocity".into());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PhysicsSettings {
    pub solver: SolverMode,
    pub gravity: Vec3,
    pub substeps: u32,
    pub iterations: u32,
    pub cell_size: f32,
    pub sleep: SleepSettings,
    pub warm_start: bool,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SolverMode {
    #[default]
    Pgs,
    /// Experimental local-anchor reuse, not an accuracy-equivalent PGS replacement.
    Tgs,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SleepSettings {
    pub enabled: bool,
    pub linear_threshold: f32,
    pub angular_threshold: f32,
    pub idle_seconds: f32,
}
impl Default for SleepSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            linear_threshold: 0.12,
            angular_threshold: 0.15,
            idle_seconds: 1.,
        }
    }
}
impl Default for PhysicsSettings {
    fn default() -> Self {
        Self {
            solver: SolverMode::Pgs,
            gravity: Vec3::new(0., 0., -9.81),
            substeps: 4,
            iterations: 12,
            cell_size: 2.,
            sleep: SleepSettings::default(),
            warm_start: true,
        }
    }
}
impl PhysicsSettings {
    /// Experimental preset. Explicit substep/iteration overrides remain validated.
    pub fn tgs() -> Self {
        Self {
            solver: SolverMode::Tgs,
            substeps: 8,
            iterations: 4,
            ..Self::default()
        }
    }
    pub fn validate(&self) -> Result<(), String> {
        if !self.cell_size.is_finite()
            || !(0.1..=1000.).contains(&self.cell_size)
            || [self.sleep.linear_threshold, self.sleep.angular_threshold]
                .iter()
                .any(|v| !v.is_finite() || !(0.0..=10.).contains(v))
            || !self.sleep.idle_seconds.is_finite()
            || !(0.1..=60.).contains(&self.sleep.idle_seconds)
        {
            return Err("invalid collision cell size or sleep thresholds".into());
        }
        if !bounded(self.gravity, 1000.)
            || !(1..=32).contains(&self.substeps)
            || !(1..=64).contains(&self.iterations)
        {
            return Err("physics requires finite gravity within +/-1000, 1..32 substeps, and 1..64 iterations".into());
        }
        Ok(())
    }
}
pub(crate) fn bounded(v: Vec3, limit: f32) -> bool {
    v.finite() && v.x.abs() <= limit && v.y.abs() <= limit && v.z.abs() <= limit
}

/// One solved contact for this world tick. Normal points from a to b.
#[derive(Clone, Copy, Debug)]
pub struct ContactEvent {
    pub a: u64,
    pub b: u64,
    pub point: Vec3,
    pub normal: Vec3,
    pub impulse: f32,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct PhysicsStats {
    pub broad_phase_builds: usize,
    pub narrow_phase_calls: usize,
    pub reused_contact_points: usize,
    pub refreshed_manifolds: usize,
    pub bound_escape_rebuilds: usize,
    pub peak_anchor_bytes: usize,
    pub solver_visits: usize,
    pub bodies: usize,
    pub moving: usize,
    pub candidate_pairs: usize,
    pub contacts: usize,
    pub step_seconds: f64,
    pub awake: usize,
    pub sleeping: usize,
    pub islands: usize,
    pub woken: usize,
    pub pair_checks: usize,
    pub integrated: usize,
    pub collision_seconds: f64,
    pub resolution_seconds: f64,
    pub warm_start_hits: usize,
    pub warm_start_misses: usize,
    pub cached_contacts: usize,
    pub peak_cached_contacts: usize,
    pub warm_start_seconds: f64,
    pub max_penetration: f32,
    pub preparation_seconds: f64,
    pub iteration_seconds: f64,
    pub contact_record_bytes: usize,
    pub prepared_record_bytes: usize,
    pub peak_contact_working_bytes: usize,
    pub prepared_capacity_bytes: usize,
}

#[derive(Default)]
pub(crate) struct Runtime {
    pub(crate) diagnostics: Option<ContactDiagnostics>,
    grid: broad_phase::Grid,
    edges: Vec<(usize, usize)>,
    cache: warm_start::Cache,
    prepared: Vec<solver::PreparedContact>,
}
impl Runtime {
    pub(crate) fn invalidate_contacts(&mut self) {
        self.cache.clear();
    }
    pub(crate) fn invalidate_body_contacts(&mut self, id: u64) {
        self.cache.invalidate_body(id);
    }
}

#[derive(Clone)]
struct Body {
    item: usize,
    id: u64,
    center: Vec3,
    rotation: Rotation,
    collider: Collider,
    inverse_mass: f32,
    inverse_inertia: Vec3,
    velocity: Vec3,
    omega: Vec3,
    kind: BodyKind,
    gravity_scale: f32,
    damping: f32,
    angular_damping: f32,
    activity: Activity,
    kinematic_moving: bool,
    disturbed: bool,
}
impl Body {
    fn active(&self) -> bool {
        match self.kind {
            BodyKind::Static => false,
            BodyKind::Dynamic => self.activity != Activity::Sleeping,
            BodyKind::Kinematic => self.kinematic_moving,
        }
    }
    fn from_item(index: usize, item: &Item) -> Self {
        let p = item.physics_body.as_ref().expect("validated body index");
        let collider = item.collider.expect("validated body requires collider");
        let inverse_mass = if p.kind == BodyKind::Dynamic {
            1. / p.mass
        } else {
            0.
        };
        let inverse_inertia = match collider.shape {
            ColliderShape::Sphere { radius } => {
                let i = 2.5 * inverse_mass / (radius * radius);
                Vec3::new(i, i, i)
            }
            ColliderShape::Box { half_extents: h } => Vec3::new(
                3. * inverse_mass / (h.y * h.y + h.z * h.z),
                3. * inverse_mass / (h.x * h.x + h.z * h.z),
                3. * inverse_mass / (h.x * h.x + h.y * h.y),
            ),
        };
        Self {
            item: index,
            id: item.id,
            center: item.transform.anchor + item.transform.rotation.rotate(collider.offset),
            rotation: item.transform.rotation,
            collider,
            inverse_mass,
            inverse_inertia,
            velocity: p.velocity,
            omega: p.angular_velocity,
            kind: p.kind,
            gravity_scale: p.gravity_scale,
            damping: p.linear_damping,
            angular_damping: p.angular_damping,
            activity: p.activity,
            kinematic_moving: p.kind == BodyKind::Kinematic
                && p.target.is_some_and(|(anchor, rotation)| {
                    anchor != item.transform.anchor || rotation != item.transform.rotation
                }),
            disturbed: false,
        }
    }
    fn inertia(&self, v: Vec3) -> Vec3 {
        // Isotropic inertia is orientation-independent: R * (sI) * R^-1 = sI.
        if self.inverse_inertia.x == self.inverse_inertia.y
            && self.inverse_inertia.y == self.inverse_inertia.z
        {
            return v.scaled(self.inverse_inertia.x);
        }
        let local = self.rotation.inverse_rotate(v);
        self.rotation.rotate(Vec3::new(
            local.x * self.inverse_inertia.x,
            local.y * self.inverse_inertia.y,
            local.z * self.inverse_inertia.z,
        ))
    }
    fn impulse(&mut self, impulse: Vec3, arm: Vec3) {
        self.velocity = self.velocity + impulse.scaled(self.inverse_mass);
        self.omega = self.omega + self.inertia(arm.cross(impulse));
    }
    fn bounds(&self) -> Bounds {
        let ext = match self.collider.shape {
            ColliderShape::Sphere { radius } => Vec3::new(radius, radius, radius),
            ColliderShape::Box { half_extents: h } => {
                let x = self.rotation.rotate(Vec3::new(h.x, 0., 0.));
                let y = self.rotation.rotate(Vec3::new(0., h.y, 0.));
                let z = self.rotation.rotate(Vec3::new(0., 0., h.z));
                Vec3::new(
                    x.x.abs() + y.x.abs() + z.x.abs(),
                    x.y.abs() + y.y.abs() + z.y.abs(),
                    x.z.abs() + y.z.abs() + z.z.abs(),
                )
            }
        };
        Bounds {
            min: self.center - ext,
            max: self.center + ext,
        }
    }
}

pub(crate) fn apply_impulse(item: &mut Item, impulse: Vec3, point: Vec3) -> bool {
    if !bounded(impulse, 1e7)
        || !bounded(point, 1e6)
        || !item
            .physics_body
            .as_ref()
            .is_some_and(|b| b.kind == BodyKind::Dynamic)
    {
        return false;
    }
    let mut body = Body::from_item(0, item);
    body.impulse(impulse, point - body.center);
    if !bounded(body.velocity, 10000.) || !bounded(body.omega, 1000.) {
        return false;
    }
    let target = item.physics_body.as_mut().unwrap();
    target.velocity = body.velocity;
    target.angular_velocity = body.omega;
    target.wake();
    true
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct BodyIndex(u32);
impl TryFrom<usize> for BodyIndex {
    type Error = &'static str;
    fn try_from(value: usize) -> Result<Self, Self::Error> {
        u32::try_from(value)
            .map(Self)
            .map_err(|_| "physics body index exceeds u32 range")
    }
}
impl BodyIndex {
    fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Clone)]
struct Constraint {
    a: BodyIndex,
    b: BodyIndex,
    point: Vec3,
    normal: Vec3,
    bias: f32,
    friction: f32,
    normal_impulse: f32,
    tangent_impulse: Vec3,
}

/// Solves against scratch state, committing only finite, supported results.
pub(crate) fn step(
    items: &mut [Item],
    indices: &[usize],
    settings: PhysicsSettings,
    dt: f32,
    runtime: &mut Runtime,
) -> Result<(PhysicsStats, Vec<ContactEvent>), String> {
    let start = std::time::Instant::now();
    if let Some(diagnostics) = &mut runtime.diagnostics {
        diagnostics.begin_tick(settings.iterations);
    }
    if let Some(last) = indices.len().checked_sub(1) {
        BodyIndex::try_from(last)?;
    }
    let mut bodies: Vec<_> = indices
        .iter()
        .map(|&i| Body::from_item(i, &items[i]))
        .collect();
    let initial = bodies.clone();
    let mut cache = std::mem::take(&mut runtime.cache);
    let mut prepared = std::mem::take(&mut runtime.prepared);
    if !settings.warm_start {
        cache.clear();
    }
    runtime.grid.configure(bodies.len(), settings.cell_size);
    let links = activity::links(bodies.len(), &runtime.edges);
    let mut active: Vec<_> = bodies
        .iter()
        .enumerate()
        .filter_map(|(i, b)| b.active().then_some(i))
        .collect();
    let mut stats = PhysicsStats {
        bodies: bodies.len(),
        moving: bodies.iter().filter(|b| b.kind != BodyKind::Static).count(),
        contact_record_bytes: std::mem::size_of::<Constraint>(),
        prepared_record_bytes: std::mem::size_of::<solver::PreparedContact>(),
        ..PhysicsStats::default()
    };
    let mut events = Vec::new();
    for i in 0..bodies.len() {
        if !settings.sleep.enabled && bodies[i].activity == Activity::Sleeping {
            stats.woken += activity::wake_connected(i, &mut bodies, &links, &mut active);
        }
    }
    let mut cursor = 0;
    while cursor < active.len() {
        let id = active[cursor];
        cursor += 1;
        for &other in &links[id] {
            stats.woken += activity::wake_connected(other, &mut bodies, &links, &mut active);
        }
    }
    let mut edges: Vec<_> = runtime
        .edges
        .iter()
        .copied()
        .filter(|&(a, b)| !bodies[a].active() && !bodies[b].active())
        .collect();
    match settings.solver {
        SolverMode::Pgs => {
            let h = dt / settings.substeps as f32;
            for substep in 0..settings.substeps {
                for (i, body) in bodies.iter_mut().enumerate() {
                    match body.kind {
                        BodyKind::Static => {}
                        BodyKind::Dynamic => {
                            if body.activity == Activity::Sleeping {
                                continue;
                            }
                            stats.integrated += 1;
                            body.velocity = (body.velocity
                                + settings.gravity.scaled(body.gravity_scale * h))
                            .scaled(1. / (1. + body.damping * h));
                            body.omega = body.omega.scaled(1. / (1. + body.angular_damping * h));
                            body.center = body.center + body.velocity.scaled(h);
                            body.rotation = body.rotation.integrate(body.omega, h)?;
                        }
                        BodyKind::Kinematic => {
                            if let Some((anchor, rotation)) =
                                items[body.item].physics_body.as_ref().unwrap().target
                            {
                                let center = anchor + rotation.rotate(body.collider.offset);
                                body.velocity = (center - initial[i].center).scaled(1. / dt);
                                body.omega = initial[i].rotation.angular_velocity_to(rotation, dt);
                                let alpha = (substep + 1) as f32 / settings.substeps as f32;
                                body.center =
                                    initial[i].center.scaled(1. - alpha) + center.scaled(alpha);
                                body.rotation = initial[i].rotation.interpolate(rotation, alpha);
                            } else {
                                body.velocity = Vec3::default();
                                body.omega = Vec3::default();
                            }
                        }
                    }
                }
                stats.broad_phase_builds += 1;
                let collision_start = std::time::Instant::now();
                let bounds: Vec<_> = bodies.iter().map(Body::bounds).collect();
                for (i, &bounds) in bounds.iter().enumerate() {
                    if !bounds.valid() || !bounded(bodies[i].center, 1.1e6) {
                        return Err("physics bounds outside supported numeric range".into());
                    }
                    runtime.grid.update(i, bounds);
                }
                let mut constraints = Vec::new();
                let mut queue: Vec<_> = bodies
                    .iter()
                    .enumerate()
                    .filter_map(|(i, b)| b.active().then_some(i))
                    .collect();
                let mut seen = std::collections::HashSet::new();
                let mut neighbors = Vec::new();
                let mut cursor = 0;
                while cursor < queue.len() {
                    let id = queue[cursor];
                    cursor += 1;
                    runtime.grid.query(id, &mut neighbors);
                    for &other in &neighbors {
                        let (a, b) = (id.min(other), id.max(other));
                        if !seen.insert((a, b)) {
                            continue;
                        }
                        stats.pair_checks += 1;
                        if !broad_phase::overlaps(bounds[a], bounds[b]) {
                            continue;
                        }
                        let (aa, ab) = (&bodies[a], &bodies[b]);
                        if aa.inverse_mass + ab.inverse_mass == 0.
                            || aa.collider.memberships & ab.collider.filter == 0
                            || ab.collider.memberships & aa.collider.filter == 0
                        {
                            continue;
                        }
                        stats.candidate_pairs += 1;
                        stats.narrow_phase_calls += 1;
                        let contacts = collision::contacts(aa, ab);
                        if contacts.is_empty() {
                            continue;
                        }
                        edges.push((a, b));
                        // Wake the whole previously settled contact group before solving,
                        // and query those newly active neighbors in this same substep.
                        stats.woken += activity::wake_connected(a, &mut bodies, &links, &mut queue);
                        stats.woken += activity::wake_connected(b, &mut bodies, &links, &mut queue);
                        let (aa, ab) = (&bodies[a], &bodies[b]);
                        for contact in contacts {
                            stats.max_penetration = stats.max_penetration.max(contact.depth);
                            let ra = contact.point - aa.center;
                            let rb = contact.point - ab.center;
                            let speed = (ab.velocity + ab.omega.cross(rb)
                                - aa.velocity
                                - aa.omega.cross(ra))
                            .dot(contact.normal);
                            let bounce = if speed < -1. {
                                -speed * aa.collider.restitution.max(ab.collider.restitution)
                            } else {
                                0.
                            };
                            let bias = (0.15 * (contact.depth - 0.002).max(0.) / h)
                                .min(3.)
                                .max(bounce);
                            constraints.push(Constraint {
                                a: BodyIndex::try_from(a)?,
                                b: BodyIndex::try_from(b)?,
                                point: contact.point,
                                normal: contact.normal,
                                bias,
                                friction: (aa.collider.friction * ab.collider.friction).sqrt(),
                                normal_impulse: 0.,
                                tangent_impulse: Vec3::default(),
                            });
                        }
                    }
                }
                constraints.sort_by_key(|c| (c.a, c.b));
                stats.collision_seconds += collision_start.elapsed().as_secs_f64();
                stats.contacts += constraints.len();
                stats.solver_visits += constraints.len() * settings.iterations as usize;
                let resolution_start = std::time::Instant::now();
                let preparation_start = std::time::Instant::now();
                solver::prepare(&bodies, &constraints, &mut prepared);
                stats.preparation_seconds += preparation_start.elapsed().as_secs_f64();
                stats.peak_contact_working_bytes = stats.peak_contact_working_bytes.max(
                    constraints.len() * (stats.contact_record_bytes + stats.prepared_record_bytes),
                );
                if settings.warm_start {
                    let start = std::time::Instant::now();
                    let hits = cache.restore(&mut bodies, &mut constraints, h);
                    stats.warm_start_hits += hits;
                    stats.warm_start_misses += constraints.len() - hits;
                    stats.warm_start_seconds += start.elapsed().as_secs_f64();
                }
                let iteration_start = std::time::Instant::now();
                match &mut runtime.diagnostics {
                    Some(diagnostics) => {
                        diagnostics::solve(&mut bodies, &mut constraints, &prepared, diagnostics)
                    }
                    None => solver::solve(
                        &mut bodies,
                        &mut constraints,
                        &prepared,
                        settings.iterations,
                    ),
                }
                stats.iteration_seconds += iteration_start.elapsed().as_secs_f64();
                for body in &mut bodies {
                    body.disturbed |= body.velocity.dot(body.velocity)
                        > settings.sleep.linear_threshold.powi(2)
                        || body.omega.dot(body.omega) > settings.sleep.angular_threshold.powi(2);
                }
                if settings.warm_start {
                    let start = std::time::Instant::now();
                    stats.cached_contacts = cache.replace(&bodies, &constraints, h);
                    stats.peak_cached_contacts =
                        stats.peak_cached_contacts.max(stats.cached_contacts);
                    stats.warm_start_seconds += start.elapsed().as_secs_f64();
                }
                for c in constraints {
                    if c.normal_impulse > 0. {
                        events.push(ContactEvent {
                            a: bodies[c.a.index()].id,
                            b: bodies[c.b.index()].id,
                            point: c.point,
                            normal: c.normal,
                            impulse: c.normal_impulse,
                        });
                    }
                }
                stats.resolution_seconds += resolution_start.elapsed().as_secs_f64();
            }
        }
        SolverMode::Tgs => tgs::Step {
            bodies: &mut bodies,
            initial: &initial,
            items,
            settings,
            dt,
            runtime,
            cache: &mut cache,
            prepared: &mut prepared,
            stats: &mut stats,
            events: &mut events,
            edges: &mut edges,
            links: &links,
        }
        .run()?,
    }
    edges.sort_unstable();
    edges.dedup();
    activity::settle(&mut bodies, &edges, settings, dt, &mut stats);
    for b in &bodies {
        let anchor = b.center - b.rotation.rotate(b.collider.offset);
        if !bounded(anchor, 1e6) || !bounded(b.velocity, 10000.) || !bounded(b.omega, 1000.) {
            return Err(format!(
                "physics result outside supported numeric range for item {}",
                b.id
            ));
        }
    }
    for b in bodies {
        let item = &mut items[b.item];
        item.transform.snap();
        item.transform.anchor = b.center - b.rotation.rotate(b.collider.offset);
        item.transform.rotation = b.rotation;
        let state = item.physics_body.as_mut().unwrap();
        state.velocity = b.velocity;
        state.angular_velocity = b.omega;
        state.target = None;
        state.activity = b.activity;
    }
    runtime.edges = edges;
    runtime.cache = cache;
    stats.prepared_capacity_bytes = prepared.capacity() * stats.prepared_record_bytes;
    runtime.prepared = prepared;
    stats.step_seconds = start.elapsed().as_secs_f64();
    Ok((stats, events))
}
fn effective_mass(a: &Body, b: &Body, ra: Vec3, rb: Vec3, n: Vec3) -> f32 {
    a.inverse_mass
        + b.inverse_mass
        + n.dot(a.inertia(ra.cross(n)).cross(ra) + b.inertia(rb.cross(n)).cross(rb))
}
fn pair_mut(values: &mut [Body], a: usize, b: usize) -> (&mut Body, &mut Body) {
    if a < b {
        let (left, right) = values.split_at_mut(b);
        (&mut left[a], &mut right[0])
    } else {
        let (left, right) = values.split_at_mut(a);
        (&mut right[0], &mut left[b])
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct PhysicsOverlapAudit {
    pub max_penetration: f32,
    pub overlapping_pairs: usize,
    pub contact_points: usize,
}
pub(crate) fn overlap_audit(
    items: &[Item],
    indices: &[usize],
    cell_size: f32,
) -> PhysicsOverlapAudit {
    let bodies: Vec<_> = indices
        .iter()
        .map(|&i| Body::from_item(i, &items[i]))
        .collect();
    let mut grid = broad_phase::Grid::default();
    grid.configure(bodies.len(), cell_size);
    let bounds: Vec<_> = bodies.iter().map(Body::bounds).collect();
    for (i, &bounds) in bounds.iter().enumerate() {
        grid.update(i, bounds);
    }
    let mut neighbors = Vec::new();
    let mut audit = PhysicsOverlapAudit::default();
    for (i, a) in bodies.iter().enumerate() {
        grid.query(i, &mut neighbors);
        for &j in &neighbors {
            if j <= i {
                continue;
            }
            let b = &bodies[j];
            if a.inverse_mass + b.inverse_mass == 0.
                || a.collider.memberships & b.collider.filter == 0
                || b.collider.memberships & a.collider.filter == 0
                || !broad_phase::overlaps(bounds[i], bounds[j])
            {
                continue;
            }
            let points = collision::contacts(a, b);
            audit.overlapping_pairs += usize::from(!points.is_empty());
            audit.contact_points += points.len();
            for p in points {
                audit.max_penetration = audit.max_penetration.max(p.depth);
            }
        }
    }
    audit
}
