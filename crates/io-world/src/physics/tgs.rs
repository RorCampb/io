//! Experimental temporal stepping with frame-local manifolds and guarded reuse.
use super::*;
use std::collections::{BTreeMap, HashSet};

const MARGIN: f32 = 0.02;
const ROTATION_DOT: f32 = 0.9996875; // About 0.05 radians since manifold construction.

struct Anchor {
    local_a: Vec3,
    local_b: Vec3,
    normal: Vec3,
    separation_offset: f32,
    normal_impulse: f32,
    tangent_impulse: Vec3,
    impact_pending: bool,
}
struct Pair {
    a: BodyIndex,
    b: BodyIndex,
    anchors: Vec<Anchor>,
    relative: Vec3,
    rotation_a: Rotation,
    rotation_b: Rotation,
}
fn length(v: Vec3) -> f32 {
    v.dot(v).sqrt()
}
fn radius(b: &Body) -> f32 {
    match b.collider.shape {
        ColliderShape::Sphere { radius } => radius,
        ColliderShape::Box { half_extents } => length(half_extents),
    }
}
fn tolerance(a: &Body, b: &Body) -> f32 {
    let size = |body: &Body| match body.collider.shape {
        ColliderShape::Sphere { radius } => radius,
        ColliderShape::Box { half_extents: h } => h.x.min(h.y).min(h.z),
    };
    (0.05 * size(a).min(size(b))).min(MARGIN)
}
fn expanded(bounds: Bounds, margin: f32) -> Bounds {
    let d = Vec3::new(margin, margin, margin);
    Bounds {
        min: bounds.min - d,
        max: bounds.max + d,
    }
}
fn contains(outer: Bounds, inner: Bounds) -> bool {
    outer.min.x <= inner.min.x
        && outer.min.y <= inner.min.y
        && outer.min.z <= inner.min.z
        && outer.max.x >= inner.max.x
        && outer.max.y >= inner.max.y
        && outer.max.z >= inner.max.z
}
fn rotated_far(a: Rotation, b: Rotation) -> bool {
    a.xyzw()
        .into_iter()
        .zip(b.xyzw())
        .map(|(x, y)| x * y)
        .sum::<f32>()
        .abs()
        < ROTATION_DOT
}
impl Anchor {
    fn arms(&self, a: &Body, b: &Body) -> (Vec3, Vec3) {
        (
            a.rotation.rotate(self.local_a),
            b.rotation.rotate(self.local_b),
        )
    }
    fn separation(&self, a: &Body, b: &Body) -> f32 {
        let (ra, rb) = self.arms(a, b);
        (b.center - a.center + rb - ra).dot(self.normal) + self.separation_offset
    }
}
impl Pair {
    fn new(a: usize, b: usize, bodies: &[Body]) -> Result<Self, String> {
        Ok(Self {
            a: BodyIndex::try_from(a)?,
            b: BodyIndex::try_from(b)?,
            anchors: Vec::new(),
            relative: bodies[b].center - bodies[a].center,
            rotation_a: bodies[a].rotation,
            rotation_b: bodies[b].rotation,
        })
    }
    fn refresh(&mut self, bodies: &[Body], stats: &mut PhysicsStats) {
        let (a, b) = (&bodies[self.a.index()], &bodies[self.b.index()]);
        stats.narrow_phase_calls += 1;
        stats.refreshed_manifolds += 1;
        let old = std::mem::take(&mut self.anchors);
        let mut used = vec![false; old.len()];
        let limit = tolerance(a, b).powi(2);
        for point in collision::contacts(a, b) {
            let local_a = a.rotation.inverse_rotate(point.point - a.center);
            let local_b = b.rotation.inverse_rotate(point.point - b.center);
            let ra = a.rotation.rotate(local_a);
            let rb = b.rotation.rotate(local_b);
            let mut anchor = Anchor {
                local_a,
                local_b,
                normal: point.normal,
                separation_offset: -point.depth - (b.center - a.center + rb - ra).dot(point.normal),
                normal_impulse: 0.,
                tangent_impulse: Vec3::default(),
                impact_pending: true,
            };
            let nearest = old
                .iter()
                .enumerate()
                .filter_map(|(i, previous)| {
                    let da = local_a - previous.local_a;
                    let db = local_b - previous.local_b;
                    (!used[i]
                        && previous.normal.dot(point.normal) >= 0.98
                        && da.dot(da) <= limit
                        && db.dot(db) <= limit)
                        .then_some((i, da.dot(da) + db.dot(db)))
                })
                .min_by(|a, b| a.1.total_cmp(&b.1));
            if let Some((i, _)) = nearest {
                used[i] = true;
                anchor.normal_impulse = old[i].normal_impulse;
                anchor.tangent_impulse = warm_start::clamp_friction(
                    old[i].tangent_impulse,
                    anchor.normal,
                    (a.collider.friction * b.collider.friction).sqrt() * anchor.normal_impulse,
                );
                anchor.impact_pending = old[i].impact_pending;
            }
            self.anchors.push(anchor);
        }
        self.relative = b.center - a.center;
        self.rotation_a = a.rotation;
        self.rotation_b = b.rotation;
    }
    fn stale(&self, bodies: &[Body]) -> bool {
        let (a, b) = (&bodies[self.a.index()], &bodies[self.b.index()]);
        if rotated_far(a.rotation, self.rotation_a) || rotated_far(b.rotation, self.rotation_b) {
            return true;
        }
        let Some(first) = self.anchors.first() else {
            return false;
        };
        let d = (b.center - a.center) - self.relative;
        let tangent = d - first.normal.scaled(d.dot(first.normal));
        length(tangent) > tolerance(a, b)
            || self
                .anchors
                .iter()
                .any(|p| p.separation(a, b) > 2. * MARGIN)
    }
}

pub(super) struct Step<'a> {
    pub bodies: &'a mut [Body],
    pub initial: &'a [Body],
    pub items: &'a [Item],
    pub settings: PhysicsSettings,
    pub dt: f32,
    pub runtime: &'a mut Runtime,
    pub cache: &'a mut warm_start::Cache,
    pub prepared: &'a mut Vec<solver::PreparedContact>,
    pub stats: &'a mut PhysicsStats,
    pub events: &'a mut Vec<ContactEvent>,
    pub edges: &'a mut Vec<(usize, usize)>,
    pub links: &'a [Vec<usize>],
}
impl Step<'_> {
    fn build_pairs(
        &mut self,
        bounds: &[Bounds],
        remaining: f32,
        pairs: &mut Vec<Pair>,
    ) -> Result<Vec<Bounds>, String> {
        self.stats.broad_phase_builds += 1;
        let mut envelopes = Vec::with_capacity(bounds.len());
        for (i, (&bounds, body)) in bounds.iter().zip(self.bodies.iter()).enumerate() {
            let motion = length(body.velocity) * remaining
                + radius(body) * (length(body.omega) * remaining).min(2.)
                + length(self.settings.gravity) * body.gravity_scale.abs() * remaining * remaining;
            let envelope = expanded(bounds, MARGIN + motion);
            if !envelope.valid() {
                return Err("invalid TGS candidate envelope".into());
            }
            self.runtime.grid.update(i, envelope);
            envelopes.push(envelope);
        }
        let mut old: BTreeMap<_, _> = std::mem::take(pairs)
            .into_iter()
            .map(|p| ((p.a.index(), p.b.index()), p))
            .collect();
        let mut queue: Vec<_> = self
            .bodies
            .iter()
            .enumerate()
            .filter_map(|(i, b)| b.active().then_some(i))
            .collect();
        let mut seen = HashSet::new();
        let mut neighbors = Vec::new();
        let mut cursor = 0;
        while cursor < queue.len() {
            let id = queue[cursor];
            cursor += 1;
            self.runtime.grid.query(id, &mut neighbors);
            for &other in &neighbors {
                let key = (id.min(other), id.max(other));
                if !seen.insert(key) {
                    continue;
                }
                self.stats.pair_checks += 1;
                let (a, b) = (&self.bodies[key.0], &self.bodies[key.1]);
                if !broad_phase::overlaps(envelopes[key.0], envelopes[key.1])
                    || a.inverse_mass + b.inverse_mass == 0.
                    || a.collider.memberships & b.collider.filter == 0
                    || b.collider.memberships & a.collider.filter == 0
                {
                    continue;
                }
                self.stats.candidate_pairs += 1;
                let mut pair = match old.remove(&key) {
                    Some(p) => p,
                    None => Pair::new(key.0, key.1, self.bodies)?,
                };
                if !pair.anchors.is_empty() || broad_phase::overlaps(bounds[key.0], bounds[key.1]) {
                    pair.refresh(self.bodies, self.stats);
                }
                if !pair.anchors.is_empty() {
                    self.stats.woken +=
                        activity::wake_connected(key.0, self.bodies, self.links, &mut queue);
                    self.stats.woken +=
                        activity::wake_connected(key.1, self.bodies, self.links, &mut queue);
                }
                pairs.push(pair);
            }
        }
        pairs.sort_by_key(|p| (p.a, p.b));
        Ok(envelopes)
    }

    pub fn run(mut self) -> Result<(), String> {
        let h = self.dt / self.settings.substeps as f32;
        let mut pairs = Vec::new();
        let mut envelopes: Vec<Bounds> = Vec::new();
        let mut constraints = Vec::new();
        let mut mapping = Vec::new();
        for substep in 0..self.settings.substeps {
            for (i, body) in self.bodies.iter_mut().enumerate() {
                match body.kind {
                    BodyKind::Static => {}
                    BodyKind::Dynamic if body.activity == Activity::Sleeping => {}
                    BodyKind::Dynamic => {
                        self.stats.integrated += 1;
                        body.velocity = (body.velocity
                            + self.settings.gravity.scaled(body.gravity_scale * h))
                        .scaled(1. / (1. + body.damping * h));
                        body.omega = body.omega.scaled(1. / (1. + body.angular_damping * h));
                    }
                    BodyKind::Kinematic => {
                        if let Some((anchor, rotation)) =
                            self.items[body.item].physics_body.as_ref().unwrap().target
                        {
                            let center = anchor + rotation.rotate(body.collider.offset);
                            body.velocity = (center - self.initial[i].center).scaled(1. / self.dt);
                            body.omega = self.initial[i]
                                .rotation
                                .angular_velocity_to(rotation, self.dt);
                        } else {
                            body.velocity = Vec3::default();
                            body.omega = Vec3::default();
                        }
                    }
                }
            }
            let start = std::time::Instant::now();
            let bounds: Vec<_> = self.bodies.iter().map(Body::bounds).collect();
            if bounds
                .iter()
                .zip(self.bodies.iter())
                .any(|(b, body)| !b.valid() || !bounded(body.center, 1.1e6))
            {
                return Err("TGS bounds outside supported numeric range".into());
            }
            let rebuild = envelopes.len() != bounds.len()
                || envelopes
                    .iter()
                    .zip(&bounds)
                    .any(|(&outer, &inner)| !contains(outer, inner));
            if rebuild {
                if substep > 0 {
                    self.stats.bound_escape_rebuilds += 1;
                }
                envelopes = self.build_pairs(&bounds, self.dt - substep as f32 * h, &mut pairs)?;
            } else {
                let mut awakened = Vec::new();
                for pair in &mut pairs {
                    let refresh = if pair.anchors.is_empty() {
                        broad_phase::overlaps(bounds[pair.a.index()], bounds[pair.b.index()])
                    } else {
                        pair.stale(self.bodies)
                    };
                    if refresh {
                        pair.refresh(self.bodies, self.stats);
                        if !pair.anchors.is_empty() {
                            self.stats.woken += activity::wake_connected(
                                pair.a.index(),
                                self.bodies,
                                self.links,
                                &mut awakened,
                            );
                            self.stats.woken += activity::wake_connected(
                                pair.b.index(),
                                self.bodies,
                                self.links,
                                &mut awakened,
                            );
                        }
                    } else {
                        self.stats.reused_contact_points += pair.anchors.len();
                    }
                }
                if !awakened.is_empty() {
                    envelopes =
                        self.build_pairs(&bounds, self.dt - substep as f32 * h, &mut pairs)?;
                }
            }
            self.stats.peak_anchor_bytes = self.stats.peak_anchor_bytes.max(
                pairs
                    .iter()
                    .map(|p| p.anchors.len() * std::mem::size_of::<Anchor>())
                    .sum(),
            );
            self.stats.collision_seconds += start.elapsed().as_secs_f64();

            let resolution_start = std::time::Instant::now();
            let start = std::time::Instant::now();
            constraints.clear();
            mapping.clear();
            self.prepared.clear();
            for (pair_id, pair) in pairs.iter_mut().enumerate() {
                let (a, b) = (&self.bodies[pair.a.index()], &self.bodies[pair.b.index()]);
                for (point_id, point) in pair.anchors.iter_mut().enumerate() {
                    let (ra, rb) = point.arms(a, b);
                    let separation =
                        (b.center - a.center + rb - ra).dot(point.normal) + point.separation_offset;
                    self.stats.max_penetration = self.stats.max_penetration.max(-separation);
                    let velocity = b.velocity + b.omega.cross(rb) - a.velocity - a.omega.cross(ra);
                    let speed = velocity.dot(point.normal);
                    let mut bounce = 0.;
                    if point.impact_pending && separation <= 0.002 {
                        if speed < -1. {
                            bounce = -speed * a.collider.restitution.max(b.collider.restitution);
                        }
                        point.impact_pending = false;
                    }
                    let bias = if separation > 0. {
                        -separation / h
                    } else {
                        (0.15 * (-separation - 0.002).max(0.) / h).min(3.)
                    }
                    .max(bounce);
                    let friction = if separation > 0.002 {
                        0.
                    } else {
                        (a.collider.friction * b.collider.friction).sqrt()
                    };
                    constraints.push(Constraint {
                        a: pair.a,
                        b: pair.b,
                        point: (a.center + ra + b.center + rb).scaled(0.5),
                        normal: point.normal,
                        bias,
                        friction,
                        normal_impulse: if self.settings.warm_start {
                            point.normal_impulse
                        } else {
                            0.
                        },
                        tangent_impulse: if self.settings.warm_start {
                            warm_start::clamp_friction(
                                point.tangent_impulse,
                                point.normal,
                                friction * point.normal_impulse,
                            )
                        } else {
                            Vec3::default()
                        },
                    });
                    self.prepared.push(solver::PreparedContact {
                        ra,
                        rb,
                        normal_response: effective_mass(a, b, ra, rb, point.normal),
                    });
                    mapping.push((pair_id, point_id));
                }
            }
            self.stats.preparation_seconds += start.elapsed().as_secs_f64();
            self.stats.contacts += constraints.len();
            self.stats.solver_visits += constraints.len() * self.settings.iterations as usize;
            self.stats.peak_contact_working_bytes = self.stats.peak_contact_working_bytes.max(
                constraints.len()
                    * (self.stats.contact_record_bytes + self.stats.prepared_record_bytes),
            );
            if self.settings.warm_start {
                let start = std::time::Instant::now();
                if substep == 0 {
                    let hits = self.cache.seed(self.bodies, &mut constraints, h);
                    self.stats.warm_start_hits += hits;
                    self.stats.warm_start_misses += constraints.len() - hits;
                }
                for (c, p) in constraints.iter().zip(self.prepared.iter()) {
                    let (a, b) = pair_mut(self.bodies, c.a.index(), c.b.index());
                    let impulse = c.normal.scaled(c.normal_impulse) + c.tangent_impulse;
                    a.impulse(impulse.scaled(-1.), p.ra);
                    b.impulse(impulse, p.rb);
                }
                self.stats.warm_start_seconds += start.elapsed().as_secs_f64();
            }
            let start = std::time::Instant::now();
            match &mut self.runtime.diagnostics {
                Some(d) => diagnostics::solve(self.bodies, &mut constraints, self.prepared, d),
                None => solver::solve(
                    self.bodies,
                    &mut constraints,
                    self.prepared,
                    self.settings.iterations,
                ),
            }
            self.stats.iteration_seconds += start.elapsed().as_secs_f64();
            for (c, &(pair_id, point_id)) in constraints.iter().zip(&mapping) {
                let anchor = &mut pairs[pair_id].anchors[point_id];
                anchor.normal_impulse = c.normal_impulse;
                anchor.tangent_impulse = c.tangent_impulse;
                if c.normal_impulse > 0. {
                    self.edges.push((c.a.index(), c.b.index()));
                    self.events.push(ContactEvent {
                        a: self.bodies[c.a.index()].id,
                        b: self.bodies[c.b.index()].id,
                        point: c.point,
                        normal: c.normal,
                        impulse: c.normal_impulse,
                    });
                }
            }
            self.stats.resolution_seconds += resolution_start.elapsed().as_secs_f64();
            for (i, body) in self.bodies.iter_mut().enumerate() {
                body.disturbed |= body.velocity.dot(body.velocity)
                    > self.settings.sleep.linear_threshold.powi(2)
                    || body.omega.dot(body.omega) > self.settings.sleep.angular_threshold.powi(2);
                match body.kind {
                    BodyKind::Dynamic if body.activity != Activity::Sleeping => {
                        body.center = body.center + body.velocity.scaled(h);
                        body.rotation = body.rotation.integrate(body.omega, h)?;
                    }
                    BodyKind::Kinematic => {
                        if let Some((anchor, rotation)) =
                            self.items[body.item].physics_body.as_ref().unwrap().target
                        {
                            let alpha = (substep + 1) as f32 / self.settings.substeps as f32;
                            let center = anchor + rotation.rotate(body.collider.offset);
                            body.center =
                                self.initial[i].center.scaled(1. - alpha) + center.scaled(alpha);
                            body.rotation = self.initial[i].rotation.interpolate(rotation, alpha);
                        }
                    }
                    _ => {}
                }
            }
        }
        if self.settings.warm_start {
            let start = std::time::Instant::now();
            for (c, &(pair_id, point_id)) in constraints.iter_mut().zip(&mapping) {
                let point = &pairs[pair_id].anchors[point_id];
                let (a, b) = (&self.bodies[c.a.index()], &self.bodies[c.b.index()]);
                let (ra, rb) = point.arms(a, b);
                c.point = (a.center + ra + b.center + rb).scaled(0.5);
            }
            self.stats.cached_contacts = self.cache.replace(self.bodies, &constraints, h);
            self.stats.peak_cached_contacts = self.stats.cached_contacts;
            let elapsed = start.elapsed().as_secs_f64();
            self.stats.warm_start_seconds += elapsed;
            self.stats.resolution_seconds += elapsed;
        }
        Ok(())
    }
}
